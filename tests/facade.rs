//! Facade tests (M2: I01, I02, I06, I07).
//!
//! `INSTA_UPDATE=no` semantics: set in-process before the first Insta call (Insta
//! memoizes tool config per binary), so failing assertions write no pendings and
//! nothing is ever blessed. All snapshot/evidence dirs are tempdirs; nothing touches
//! `tests/snapshots`. Insta dedups repeat names per process (`name-2`), so every test
//! uses unique snapshot names.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use tuisnap::assert::{
    assert_frozen_screenshot, assert_frozen_snapshot, check_consistent, check_frozen_screenshot,
    check_frozen_snapshot, emit_four, frozen_accept, generation_id, import_frozen_v1,
    png_generation, png_tag_generation, render_sample, FrozenError, ImportError,
};
use tuisnap::insta_proto::insta_string;
use tuisnap::{Cell, Color, Cursor, CursorStyle, Mods, Rgb, Screen};

struct Workspace {
    _tmp: tempfile::TempDir,
    snaps: PathBuf,
    evidence: PathBuf,
}

/// Shared hermetic dirs for the whole binary (set once: env is process-global and
/// tests run in parallel threads, so per-test env would race).
fn workspace() -> &'static Workspace {
    static O: OnceLock<Workspace> = OnceLock::new();
    O.get_or_init(|| {
        std::env::set_var("INSTA_UPDATE", "no");
        let tmp = tempfile::Builder::new()
            .prefix("facade-")
            .tempdir()
            .unwrap();
        let snaps = tmp.path().join("snaps");
        let evidence = tmp.path().join("evidence");
        fs::create_dir(&snaps).unwrap();
        fs::create_dir(&evidence).unwrap();
        std::env::set_var("TUISNAP_SNAPSHOT_DIR", &snaps);
        std::env::set_var("TUISNAP_EVIDENCE_DIR", &evidence);
        Workspace {
            _tmp: tmp,
            snaps,
            evidence,
        }
    })
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

#[test]
fn snapshot_macro_passes_on_identical_rerun() {
    let ws = workspace();
    let screen = fixture();
    let canonical = insta_string(&screen);
    let gen = generation_id(&canonical);
    // Second same-process call auto-suffixes to `fac_rerun-2` (no public opt-out).
    write_text_snap(&ws.snaps, "fac_rerun", &gen, &canonical);
    write_text_snap(&ws.snaps, "fac_rerun-2", &gen, &canonical);
    tuisnap::assert_snapshot!("fac_rerun", &screen);
    tuisnap::assert_snapshot!("fac_rerun", &screen);
}

// ---------------------------------------------------------------------------
// I02: assert_screenshot!
// ---------------------------------------------------------------------------

#[test]
fn screenshot_passes_when_consistent() {
    let ws = workspace();
    let screen = fixture();
    let sample = render_sample(&screen).unwrap();
    let gen = generation_id(&sample.canonical);
    write_text_snap(&ws.snaps, "fac_shotok", &gen, &sample.canonical);
    write_binary_snap(
        &ws.snaps,
        "fac_shotok-img",
        &gen,
        &png_tag_generation(&sample.png, &gen),
    );
    tuisnap::assert_screenshot!("fac_shotok", &screen);
}

#[test]
fn screenshot_evidence_present_before_failure() {
    let ws = workspace();
    let screen = fixture();
    // No approvals for fac_evidence: the macro must fail.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        tuisnap::assert_screenshot!("fac_evidence", &screen);
    }));
    assert!(result.is_err(), "unapproved screenshot must fail");
    // ...but candidate evidence from the same sample is on disk first.
    let sample = render_sample(&screen).unwrap();
    let gen = generation_id(&sample.canonical);
    assert_eq!(
        fs::read(ws.evidence.join("fac_evidence.png")).unwrap(),
        png_tag_generation(&sample.png, &gen)
    );
    assert_eq!(
        fs::read_to_string(ws.evidence.join("fac_evidence.ansi")).unwrap(),
        sample.ansi
    );
    assert_eq!(
        fs::read_to_string(ws.evidence.join("fac_evidence.txt")).unwrap(),
        sample.txt
    );
    assert_eq!(
        fs::read_to_string(ws.evidence.join("fac_evidence.html")).unwrap(),
        sample.html
    );
    // INSTA_UPDATE=no never blesses and writes no pendings.
    assert!(!ws.snaps.join("fac_evidence.snap").exists());
    assert!(!ws.snaps.join("fac_evidence.snap.new").exists());
    assert!(!ws.snaps.join("fac_evidence-img.snap").exists());
    assert!(!ws.snaps.join("fac_evidence-img.snap.new").exists());
}

#[test]
fn screenshot_mixed_generation_fails() {
    let ws = workspace();
    let screen = fixture();
    let sample = render_sample(&screen).unwrap();
    let gen = generation_id(&sample.canonical);
    // Canonical approved at gen-A; PNG pixels identical but bound to gen-B.
    write_text_snap(&ws.snaps, "fac_mixed", &gen, &sample.canonical);
    write_binary_snap(
        &ws.snaps,
        "fac_mixed-img",
        "gen-b",
        &png_tag_generation(&sample.png, "gen-b"),
    );
    // Strict gate agrees directly.
    check_consistent(&ws.snaps, "fac_mixed", "fac_mixed-img")
        .expect_err("mixed baseline must be inconsistent");
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        tuisnap::assert_screenshot!("fac_mixed", &screen);
    }));
    let msg = panic_message(result.unwrap_err());
    assert!(msg.contains("mixed compound baseline"), "{msg}");
}

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

#[test]
fn frozen_missing_fails() {
    let root = frozen_dir();
    let screen = fixture();
    let err = check_frozen_snapshot(root.path(), "shot", &screen).unwrap_err();
    assert!(matches!(err, FrozenError::Missing { .. }), "{err}");
    let err = check_frozen_screenshot(root.path(), "shot", &screen).unwrap_err();
    assert!(matches!(err, FrozenError::Missing { .. }), "{err}");
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        assert_frozen_snapshot(root.path(), "shot", &screen);
    }))
    .is_err());
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        assert_frozen_screenshot(root.path(), "shot", &screen);
    }))
    .is_err());
}

#[test]
fn frozen_corrupt_fails_and_never_heals() {
    let root = frozen_dir();
    let screen = fixture();
    write_frozen(root.path(), "shot", &screen, false);
    fs::write(root.path().join("shot.png"), b"not a png").unwrap();
    let before = list_files(root.path());
    let err = check_frozen_screenshot(root.path(), "shot", &screen).unwrap_err();
    assert!(matches!(err, FrozenError::Corrupt { .. }), "{err}");
    assert_eq!(
        list_files(root.path()),
        before,
        "frozen failure must not write"
    );
    // Non-UTF-8 canonical is corrupt too.
    fs::write(root.path().join("shot.canonical.txt"), b"\xff\xfe invalid").unwrap();
    let err = check_frozen_snapshot(root.path(), "shot", &screen).unwrap_err();
    assert!(matches!(err, FrozenError::Corrupt { .. }), "{err}");
    assert_eq!(
        list_files(root.path()),
        before,
        "frozen failure must not write"
    );
}

#[test]
fn frozen_accept_always_errors() {
    let root = frozen_dir();
    write_frozen(root.path(), "shot", &fixture(), true);
    // Even with valid state present...
    let err = frozen_accept(root.path(), "shot").unwrap_err();
    assert!(matches!(err, FrozenError::AcceptRejected { .. }), "{err}");
    // ...and on an empty root.
    let empty = frozen_dir();
    assert!(matches!(
        frozen_accept(empty.path(), "x"),
        Err(FrozenError::AcceptRejected { .. })
    ));
}

#[test]
fn frozen_passes_when_matching() {
    let root = frozen_dir();
    let screen = fixture();
    // Untagged legacy PNG: pixel verdict stands.
    write_frozen(root.path(), "plain", &screen, false);
    check_frozen_snapshot(root.path(), "plain", &screen).unwrap();
    check_frozen_screenshot(root.path(), "plain", &screen).unwrap();
    // Tagged PNG with the right generation: full gate green.
    write_frozen(root.path(), "tagged", &screen, true);
    check_frozen_screenshot(root.path(), "tagged", &screen).unwrap();
    // Tagged PNG with the WRONG generation: pixels match, binding fails.
    let sample = render_sample(&screen).unwrap();
    fs::write(
        root.path().join("mistag.canonical.txt"),
        insta_string(&screen),
    )
    .unwrap();
    fs::write(
        root.path().join("mistag.png"),
        png_tag_generation(&sample.png, "gen-b"),
    )
    .unwrap();
    let err = check_frozen_screenshot(root.path(), "mistag", &screen).unwrap_err();
    assert!(matches!(err, FrozenError::Mismatch { .. }), "{err}");
    assert!(err.to_string().contains("generation"), "{err}");
}

// ---------------------------------------------------------------------------
// I07: four-artifact export + read-only importer
// ---------------------------------------------------------------------------

#[test]
fn emit_four_deterministic() {
    let screen = fixture();
    let a = tempfile::Builder::new()
        .prefix("facade-emit-a-")
        .tempdir()
        .unwrap();
    let b = tempfile::Builder::new()
        .prefix("facade-emit-b-")
        .tempdir()
        .unwrap();
    let pa = emit_four(&screen, a.path()).unwrap();
    let pb = emit_four(&screen, b.path()).unwrap();
    for (fa, fb) in [
        (pa.ansi, pb.ansi),
        (pa.txt, pb.txt),
        (pa.png, pb.png),
        (pa.html, pb.html),
    ] {
        assert_eq!(fs::read(&fa).unwrap(), fs::read(&fb).unwrap(), "{fa:?}");
    }
}

#[test]
fn import_frozen_v1_roundtrip_and_unsupported() {
    let screen = fixture();
    let dir = tempfile::Builder::new()
        .prefix("facade-import-")
        .tempdir()
        .unwrap();
    emit_four(&screen, dir.path()).unwrap();
    let tree = import_frozen_v1(dir.path()).unwrap();
    assert_eq!(tree.scenarios.len(), 1);
    assert_eq!(tree.scenarios[0].name, "snapshot");
    assert!(tree.unsupported.is_empty(), "{:?}", tree.unsupported);
    assert_eq!(
        tree.scenarios[0].png,
        fs::read(dir.path().join("snapshot.png")).unwrap()
    );
    assert_eq!(
        tree.scenarios[0].html,
        fs::read_to_string(dir.path().join("snapshot.html")).unwrap()
    );
    // Extra file + unknown embedded field are REPORTED, not fatal.
    fs::write(dir.path().join("notes.md"), "reviewer notes").unwrap();
    let html = fs::read_to_string(dir.path().join("snapshot.html")).unwrap();
    let doctored = html.replacen("{\"version\":", "{\"zzz\":1,\"version\":", 1);
    assert_ne!(doctored, html, "setup: embed must be rewritten");
    fs::write(dir.path().join("snapshot.html"), doctored).unwrap();
    let tree = import_frozen_v1(dir.path()).unwrap();
    assert_eq!(tree.scenarios.len(), 1);
    assert!(
        tree.unsupported.iter().any(|u| u.contains("notes.md")),
        "{:?}",
        tree.unsupported
    );
    assert!(
        tree.unsupported.iter().any(|u| u.contains("zzz")),
        "{:?}",
        tree.unsupported
    );
}

#[test]
fn import_frozen_v1_rejects_bad_trees() {
    let screen = fixture();
    // Incomplete stem: missing .txt.
    let dir = tempfile::Builder::new()
        .prefix("facade-bad-")
        .tempdir()
        .unwrap();
    emit_four(&screen, dir.path()).unwrap();
    fs::remove_file(dir.path().join("snapshot.txt")).unwrap();
    let err = import_frozen_v1(dir.path()).unwrap_err();
    assert!(matches!(err, ImportError::Incomplete { .. }), "{err}");
    // Invalid stem name (backslash is a legal unix filename char, illegal here).
    let dir2 = tempfile::Builder::new()
        .prefix("facade-bad2-")
        .tempdir()
        .unwrap();
    for ext in ["ansi", "txt", "png", "html"] {
        fs::write(dir2.path().join(format!("we\\ird.{ext}")), b"x").unwrap();
    }
    let err = import_frozen_v1(dir2.path()).unwrap_err();
    assert!(matches!(err, ImportError::InvalidName(_)), "{err}");
    // Undecodable PNG.
    let dir3 = tempfile::Builder::new()
        .prefix("facade-bad3-")
        .tempdir()
        .unwrap();
    emit_four(&screen, dir3.path()).unwrap();
    fs::write(dir3.path().join("snapshot.png"), b"junk").unwrap();
    let err = import_frozen_v1(dir3.path()).unwrap_err();
    assert!(matches!(err, ImportError::Corrupt { .. }), "{err}");
}

// ---------------------------------------------------------------------------
// Generation binding unit checks
// ---------------------------------------------------------------------------

#[test]
fn generation_binding_roundtrip() {
    assert_eq!(generation_id("x"), generation_id("x"));
    assert_ne!(generation_id("x"), generation_id("y"));
    let sample = render_sample(&fixture()).unwrap();
    let tagged = png_tag_generation(&sample.png, "g1");
    assert_eq!(png_generation(&tagged).as_deref(), Some("g1"));
    assert_eq!(png_generation(&sample.png), None);
    assert_eq!(png_generation(b"junk"), None);
    // Strict gate fails on missing bindings.
    let empty = tempfile::Builder::new()
        .prefix("facade-empty-")
        .tempdir()
        .unwrap();
    assert!(check_consistent(empty.path(), "c", "p").is_err());
}

// ---------------------------------------------------------------------------
// F2: evidence names are validated before any write
// ---------------------------------------------------------------------------

#[test]
fn screenshot_rejects_unsafe_names_before_evidence() {
    let ws = workspace();
    let screen = fixture();
    for bad in [
        "../../fac_evil",
        "/tmp/fac_evil_abs",
        "fac_nest/../fac_evil_dotdot",
    ] {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            tuisnap::assert_screenshot!(bad, &screen);
        }));
        let msg = panic_message(result.unwrap_err());
        assert!(msg.contains("invalid snapshot name"), "{bad:?}: {msg}");
    }
    // Nothing written: no fac_evil evidence, no escape above the evidence root.
    assert!(list_files(&ws.evidence).iter().all(|p| !p
        .file_name()
        .unwrap()
        .to_string_lossy()
        .contains("fac_evil")));
    let mut top: Vec<PathBuf> = list_files(ws.evidence.parent().unwrap());
    top.sort();
    assert_eq!(top, vec![ws.evidence.clone(), ws.snaps.clone()]);
}
