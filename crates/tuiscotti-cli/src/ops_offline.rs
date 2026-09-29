//! Offline commands: `inspect`, `render`, `diff`, `review`, `accept`,
//! `report`, `import`, `trace`.
//!
//! These never spawn a child and never require the `pty` feature: they work
//! in a `--no-default-features` build.
//!
//! Every stdout path goes through [`crate::write_stdout`] (buffered) or
//! [`crate::write_line`] (streaming): no `println!`, so a closed pipe is a
//! clean exit 0 instead of an EPIPE panic (exit 101).

use std::path::Path;

use tuiscotti::proto::{self, EXIT_OP_ERROR, EXIT_USAGE, EXIT_VERIFY_FAIL};

use crate::cli::{RenderFormat, TraceKind};

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
    let mut buf = String::new();
    crate::push_line(
        &mut buf,
        &format!(
            "artifacts in {} ({} files, offline view):",
            dir.display(),
            files.len()
        ),
    );
    for (name, len) in &files {
        crate::push_line(&mut buf, &format!("  {name} ({len} bytes)"));
    }
    let manifest_path = dir.join("manifest.json");
    if manifest_path.is_file() {
        match std::fs::read_to_string(&manifest_path) {
            Ok(text) => match serde_json::from_str::<serde_json::Value>(&text) {
                Ok(v) => {
                    crate::push_line(
                        &mut buf,
                        &format!(
                            "manifest: {}",
                            serde_json::to_string(&v).unwrap_or_else(|_| text.clone())
                        ),
                    );
                }
                Err(_) => {
                    crate::push_line(
                        &mut buf,
                        &format!("manifest: (not JSON, {} bytes)", text.len()),
                    );
                }
            },
            Err(e) => {
                return crate::fail_flushed(&buf, &format!("read manifest.json: {e}"));
            }
        }
    }
    let journal_path = dir.join("journal.jsonl");
    if journal_path.is_file() {
        match proto::read_journal(&journal_path) {
            Ok(events) => {
                crate::push_line(&mut buf, &format!("journal: {} events", events.len()));
            }
            Err(e) => {
                return crate::fail_flushed(&buf, &e.to_string());
            }
        }
    }
    crate::write_stdout(&buf)
}

pub fn cmd_render(
    input: &Path,
    formats: &[RenderFormat],
    out: &str,
    font_file: Option<&Path>,
) -> i32 {
    if formats.is_empty() {
        eprintln!("error: pass at least one --format (txt|ansi|json|svg|html|png)");
        return EXIT_USAGE;
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
    let fonts = match RasterFonts::load(font_file) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("error: {e}");
            return EXIT_OP_ERROR;
        }
    };
    let mut renderer: Option<tuiscotti::Renderer> = None;
    let mut buf = String::new();
    for format in formats {
        let ext = format.extension();
        let path = format!("{out}.{ext}");
        if let Some(parent) = Path::new(&path).parent() {
            if !parent.as_os_str().is_empty() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    return crate::fail_flushed(&buf, &format!("mkdir {}: {e}", parent.display()));
                }
            }
        }
        if let Err(e) = render_format_to(&frame, &fonts, &mut renderer, *format, &path) {
            return crate::fail_flushed(&buf, &format!("render {ext}: {e}"));
        }
        crate::push_line(&mut buf, &format!("wrote {path}"));
    }
    crate::write_stdout(&buf)
}

/// Raster profile plus the font bytes backing it: a `--font-file` override
/// (one file used for all four faces) or the vendored faces.
struct RasterFonts {
    profile: tuiscotti::Profile,
    owned: Option<[Vec<u8>; 4]>,
}

impl RasterFonts {
    fn load(font_file: Option<&Path>) -> Result<Self, String> {
        let Some(path) = font_file else {
            return Ok(Self {
                profile: tuiscotti::Profile::default_profile(),
                owned: None,
            });
        };
        let bytes =
            std::fs::read(path).map_err(|e| format!("read font {}: {e}", path.display()))?;
        let profile = tuiscotti::Profile::default_profile()
            .with_font_file(path.display().to_string(), &bytes);
        Ok(Self {
            profile,
            owned: Some([bytes.clone(), bytes.clone(), bytes.clone(), bytes]),
        })
    }

    fn faces(&self) -> tuiscotti::FontFaces<'_> {
        if let Some(owned) = &self.owned {
            tuiscotti::FontFaces {
                regular: owned[0].as_slice(),
                bold: owned[1].as_slice(),
                italic: owned[2].as_slice(),
                bold_italic: owned[3].as_slice(),
            }
        } else {
            tuiscotti::FontFaces {
                regular: tuiscotti::VENDORED_FONT,
                bold: tuiscotti::VENDORED_FONT_BOLD,
                italic: tuiscotti::VENDORED_FONT_ITALIC,
                bold_italic: tuiscotti::VENDORED_FONT_BOLD_ITALIC,
            }
        }
    }
}

/// Render `frame` in one `format` to `path`. The raster renderer is lazily
/// constructed and reused across formats (faces parse once).
fn render_format_to(
    frame: &tuiscotti::Frame,
    fonts: &RasterFonts,
    renderer: &mut Option<tuiscotti::Renderer>,
    format: RenderFormat,
    path: &str,
) -> Result<(), String> {
    match format {
        RenderFormat::Txt => std::fs::write(path, frame.text()).map_err(|e| e.to_string()),
        RenderFormat::Ansi => {
            std::fs::write(path, tuiscotti::render::ansi_dump(frame)).map_err(|e| e.to_string())
        }
        RenderFormat::Json => std::fs::write(path, frame.to_json()).map_err(|e| e.to_string()),
        RenderFormat::Svg => {
            std::fs::write(path, tuiscotti::render::render_svg(frame, &fonts.profile))
                .map_err(|e| e.to_string())
        }
        RenderFormat::Html => {
            let html = cached_renderer(fonts, renderer)?
                .render_html(frame, "frame")
                .map_err(|e| e.to_string())?;
            std::fs::write(path, html).map_err(|e| e.to_string())
        }
        RenderFormat::Png => {
            let rendered = cached_renderer(fonts, renderer)?
                .render(frame)
                .map_err(|e| e.to_string())?;
            std::fs::write(path, &rendered.png).map_err(|e| e.to_string())?;
            std::fs::write(format!("{path}.fidelity.json"), rendered.fidelity.to_json())
                .map_err(|e| e.to_string())
        }
    }
}

/// Lazily construct (once) and borrow the raster renderer.
fn cached_renderer<'r>(
    fonts: &RasterFonts,
    renderer: &'r mut Option<tuiscotti::Renderer>,
) -> Result<&'r mut tuiscotti::Renderer, String> {
    if renderer.is_none() {
        let faces = fonts.faces();
        let built = tuiscotti::Renderer::new(&fonts.profile, &faces).map_err(|e| e.to_string())?;
        *renderer = Some(built);
    }
    renderer
        .as_mut()
        .ok_or_else(|| "renderer build failed".to_string())
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
            let buf = format!(
                "pixels_equal={} dims_equal={} score={}\n",
                v.pixels_equal, v.dims_equal, v.score
            );
            let w = crate::write_stdout(&buf);
            if w != 0 {
                return w;
            }
            if v.pixels_equal { 0 } else { EXIT_VERIFY_FAIL }
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
    let mut buf = String::new();
    if verdicts.is_empty() {
        crate::push_line(&mut buf, &format!("no verdicts in {}", dir.display()));
        return crate::write_stdout(&buf);
    }
    let mut failed = 0u32;
    for v in &verdicts {
        if v.passed() {
            crate::push_line(&mut buf, &format!("PASS {}", v.name));
        } else {
            failed += 1;
            if v.detail.is_empty() {
                crate::push_line(&mut buf, &format!("FAIL {}", v.name));
            } else {
                crate::push_line(&mut buf, &format!("FAIL {} ({})", v.name, v.detail));
            }
        }
    }
    crate::push_line(
        &mut buf,
        &format!(
            "{} passed, {} failed",
            verdicts.len() - failed as usize,
            failed
        ),
    );
    let w = crate::write_stdout(&buf);
    if w != 0 {
        return w;
    }
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
            let buf = format!("accepted `{name}` in {}\n", store.display());
            crate::write_stdout(&buf)
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
    let buf = format!(
        "report: {} ({} verdicts, {} failed)\n",
        out.display(),
        verdicts.len(),
        failed
    );
    crate::write_stdout(&buf)
}

pub fn cmd_import(dir: &Path) -> i32 {
    match tuiscotti::assert::import_frozen_v1(dir) {
        Ok(tree) => {
            let mut buf = String::new();
            crate::push_line(&mut buf, &format!("scenarios: {}", tree.scenarios.len()));
            for s in &tree.scenarios {
                crate::push_line(&mut buf, &format!("  {}", s.name));
            }
            crate::push_line(
                &mut buf,
                &format!("unsupported: {}", tree.unsupported.len()),
            );
            for u in &tree.unsupported {
                crate::push_line(&mut buf, &format!("  {u}"));
            }
            crate::write_stdout(&buf)
        }
        Err(e) => {
            eprintln!("error: import failed: {e}");
            EXIT_OP_ERROR
        }
    }
}

pub fn cmd_trace(input: &Path, kind: Option<TraceKind>) -> i32 {
    let events = match proto::read_journal(input) {
        Ok(e) => e,
        Err(e) => return op_error(&e),
    };
    // Streaming: journals are unbounded, so emit line-by-line through the
    // EPIPE-tolerant writer instead of buffering the whole view.
    for ev in events {
        if let Some(k) = kind {
            if ev.kind != k.as_str() {
                continue;
            }
        }
        let line = format!("{} {} {}", ev.seq, ev.kind, ev.detail);
        if let Some(code) = crate::write_line(&line) {
            return code;
        }
    }
    0
}
