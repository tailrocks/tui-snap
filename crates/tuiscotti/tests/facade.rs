//! Facade tests (M2: I01, I02, I06, I07).
//!
//! Hermetic dirs via [`Policy::EvolvingIn`]: `set_var` is an `unsafe fn` in
//! edition 2024 and cannot be used under the workspace lints, so tests pass
//! tempdirs explicitly instead of `TUISNAP_SNAPSHOT_DIR` /
//! `TUISNAP_EVIDENCE_DIR`. `INSTA_UPDATE` stays ambient (read-only): green-path
//! assertions hold under every mode, while pending/no-bless assertions guard
//! on the effective mode (see `common`). All snapshot/evidence dirs are
//! tempdirs; nothing touches `tests/snapshots`. Insta dedups repeat names per
//! process (`name-2`), so every test uses unique snapshot names.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tuiscotti::assert::{Policy, generation_id, png_tag_generation, render_sample};
use tuiscotti::insta_proto::insta_string;
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
fn workspace() -> &'static Workspace {
    static O: OnceLock<Workspace> = OnceLock::new();
    O.get_or_init(|| {
        let tmp = tempfile::Builder::new()
            .prefix("facade-")
            .tempdir()
            .unwrap();
        let snaps = tmp.path().join("snaps");
        let evidence = tmp.path().join("evidence");
        fs::create_dir(&snaps).unwrap();
        fs::create_dir(&evidence).unwrap();
        Workspace {
            _tmp: tmp,
            snaps,
            evidence,
        }
    })
}

impl Workspace {
    fn policy(&self) -> Policy {
        Policy::EvolvingIn {
            snapshots: self.snaps.clone(),
            evidence: self.evidence.clone(),
        }
    }
}

fn fixture() -> Screen {
    let plain = Mods::default();
    let bold = Mods {
        bold: true,
        ..Mods::default()
    };
    let cells = vec![
        Cell {
            x: 0,
            y: 0,
            symbol: "A".to_string(),
            width: 1,
            continuation: false,
            fg: Color::Indexed(1),
            bg: Color::Default,
            mods: bold,
            underline_color: Color::Default,
        },
        Cell {
            x: 1,
            y: 0,
            symbol: "b".to_string(),
            width: 1,
            continuation: false,
            fg: Color::Rgb(Rgb::new(1, 2, 3)),
            bg: Color::Default,
            mods: plain,
            underline_color: Color::Default,
        },
        Cell {
            x: 2,
            y: 0,
            symbol: " ".to_string(),
            width: 1,
            continuation: false,
            fg: Color::Default,
            bg: Color::Indexed(4),
            mods: plain,
            underline_color: Color::Default,
        },
        Cell {
            x: 0,
            y: 1,
            symbol: "Z".to_string(),
            width: 1,
            continuation: false,
            fg: Color::Default,
            bg: Color::Default,
            mods: plain,
            underline_color: Color::Default,
        },
        Cell {
            x: 1,
            y: 1,
            symbol: "y".to_string(),
            width: 1,
            continuation: false,
            fg: Color::Default,
            bg: Color::Default,
            mods: plain,
            underline_color: Color::Default,
        },
        Cell {
            x: 2,
            y: 1,
            symbol: "x".to_string(),
            width: 1,
            continuation: false,
            fg: Color::Default,
            bg: Color::Default,
            mods: plain,
            underline_color: Color::Default,
        },
    ];
    Screen::validate(
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
    )
    .unwrap()
}

fn write_text_snap(dir: &Path, name: &str, generation: &str, body: &str) {
    let content = format!(
        "---\nsource: tests/facade.rs\ndescription: tuisnap generation {generation}\nexpression: canonical\n---\n{body}"
    );
    fs::write(dir.join(format!("{name}.snap")), content).unwrap();
}

fn write_binary_snap(dir: &Path, name: &str, generation: &str, sidecar: &[u8]) {
    let meta = format!(
        "---\nsource: tests/facade.rs\ndescription: tuisnap generation {generation}\nexpression: png_bytes\nextension: png\nsnapshot_kind: binary\n---\n"
    );
    fs::write(dir.join(format!("{name}.snap")), meta).unwrap();
    fs::write(dir.join(format!("{name}.snap.png")), sidecar).unwrap();
}

fn panic_message(p: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = p.downcast_ref::<String>() {
        s.clone()
    } else if let Some(s) = p.downcast_ref::<&str>() {
        (*s).to_string()
    } else {
        "<non-string panic>".to_string()
    }
}

fn list_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in fs::read_dir(dir).unwrap() {
        out.push(e.unwrap().path());
    }
    out.sort();
    out
}

// ---------------------------------------------------------------------------
// I01: assert_snapshot!
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// I06: frozen policy
// ---------------------------------------------------------------------------

fn frozen_dir() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("facade-frozen-")
        .tempdir()
        .unwrap()
}

fn write_frozen(root: &Path, name: &str, screen: &Screen, tag_png: bool) {
    fs::write(
        root.join(format!("{name}.canonical.txt")),
        insta_string(screen),
    )
    .unwrap();
    let sample = render_sample(screen).unwrap();
    let png = if tag_png {
        png_tag_generation(&sample.png, &generation_id(&sample.canonical))
    } else {
        sample.png
    };
    fs::write(root.join(format!("{name}.png")), png).unwrap();
}
