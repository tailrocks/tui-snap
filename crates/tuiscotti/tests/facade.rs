//! Facade tests (M2: I01, I02, I06, I07).
//!
//! Hermetic dirs via [`Policy::EvolvingIn`]: `set_var` is an `unsafe fn` in
//! edition 2024 and cannot be used under the workspace lints, so tests pass
//! tempdirs explicitly instead of `TUISCOTTI_SNAPSHOT_DIR` /
//! `TUISCOTTI_EVIDENCE_DIR`. `INSTA_UPDATE` stays ambient (read-only): green-path
//! assertions hold under every mode, while pending/no-bless assertions guard
//! on the effective mode (see `common`). All snapshot/evidence dirs are
//! tempdirs; nothing touches `tests/snapshots`. Insta dedups repeat names per
//! process (`name-2`), so every test uses unique snapshot names.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tuiscotti::assert::{
    Policy, png_tag_generation, render_identity, render_sample, sample_binding,
};
use tuiscotti::screen::canonical_string;
use tuiscotti::{Cell, Color, Cursor, CursorStyle, Mods, Rgb, Screen};

mod common;

#[path = "facade/frozen.rs"]
mod frozen;
#[path = "facade/interop.rs"]
mod interop;
#[path = "facade/screenshot.rs"]
mod screenshot;

struct Workspace {
    _tmp: tempfile::TempDir,
    snaps: PathBuf,
    evidence: PathBuf,
}

/// Shared hermetic dirs for the whole binary (unique snapshot names per test
/// keep parallel tests isolated).
fn workspace() -> Result<&'static Workspace, Box<dyn std::error::Error>> {
    static O: OnceLock<Result<Workspace, String>> = OnceLock::new();
    O.get_or_init(|| {
        (|| -> Result<Workspace, Box<dyn std::error::Error>> {
            let tmp = tempfile::Builder::new().prefix("facade-").tempdir()?;
            let snaps = tmp.path().join("snaps");
            let evidence = tmp.path().join("evidence");
            fs::create_dir(&snaps)?;
            fs::create_dir(&evidence)?;
            Ok(Workspace {
                _tmp: tmp,
                snaps,
                evidence,
            })
        })()
        .map_err(|e| e.to_string())
    })
    .as_ref()
    .map_err(|e| format!("workspace init: {e}").into())
}

impl Workspace {
    fn policy(&self) -> Policy {
        Policy::EvolvingIn {
            snapshots: self.snaps.clone(),
            evidence: self.evidence.clone(),
        }
    }
}

fn cell(x: u16, y: u16, symbol: &str, fg: Color, bg: Color, mods: Mods) -> Cell {
    Cell {
        x,
        y,
        symbol: symbol.to_string(),
        width: 1,
        continuation: false,
        fg,
        bg,
        mods,
        underline_color: Color::Default,
    }
}

fn fixture() -> Result<Screen, Box<dyn std::error::Error>> {
    let plain = Mods::default();
    let bold = Mods {
        bold: true,
        ..Mods::default()
    };
    let cells = vec![
        cell(0, 0, "A", Color::Indexed(1), Color::Default, bold),
        cell(
            1,
            0,
            "b",
            Color::Rgb(Rgb::new(1, 2, 3)),
            Color::Default,
            plain,
        ),
        cell(2, 0, " ", Color::Default, Color::Indexed(4), plain),
        cell(0, 1, "Z", Color::Default, Color::Default, plain),
        cell(1, 1, "y", Color::Default, Color::Default, plain),
        cell(2, 1, "x", Color::Default, Color::Default, plain),
    ];
    Ok(Screen::validate(
        3,
        2,
        0,
        0,
        cells,
        Cursor {
            x: 0,
            y: 0,
            visible: true,
            style: CursorStyle::Block,
            blinking: false,
        },
    )?)
}

fn write_text_snap(
    dir: &Path,
    name: &str,
    generation: &str,
    body: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let content = format!(
        "---\nsource: tests/facade.rs\ndescription: tuiscotti generation {generation}\nexpression: canonical\n---\n{body}"
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
        "---\nsource: tests/facade.rs\ndescription: tuiscotti generation {generation}\nexpression: png_bytes\nextension: png\nsnapshot_kind: binary\n---\n"
    );
    fs::write(dir.join(format!("{name}.snap")), meta)?;
    Ok(fs::write(dir.join(format!("{name}.snap.png")), sidecar)?)
}

fn panic_message(p: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = p.downcast_ref::<String>() {
        s.clone()
    } else if let Some(s) = p.downcast_ref::<&str>() {
        (*s).to_string()
    } else {
        "<non-string panic>".to_string()
    }
}

fn list_files(dir: &Path) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    let mut out = Vec::new();
    for e in fs::read_dir(dir)? {
        out.push(e?.path());
    }
    out.sort();
    Ok(out)
}

// ---------------------------------------------------------------------------
// I01: assert_snapshot!
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// I06: frozen policy
// ---------------------------------------------------------------------------

fn frozen_dir() -> Result<tempfile::TempDir, Box<dyn std::error::Error>> {
    Ok(tempfile::Builder::new()
        .prefix("facade-frozen-")
        .tempdir()?)
}

fn write_frozen(
    root: &Path,
    name: &str,
    screen: &Screen,
    tag_png: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    fs::write(
        root.join(format!("{name}.canonical.txt")),
        canonical_string(screen),
    )?;
    let sample = render_sample(screen)?;
    let png = if tag_png {
        let binding = sample_binding(&sample.canonical, &render_identity(), &sample.png);
        png_tag_generation(&sample.png, &binding)
    } else {
        sample.png
    };
    Ok(fs::write(root.join(format!("{name}.png")), png)?)
}
