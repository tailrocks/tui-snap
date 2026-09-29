use super::*;
use std::fs;
use std::path::PathBuf;
use tuiscotti::assert::{
    ImportError, check_consistent, emit_four, generation_id, import_frozen_v1, png_generation,
    png_tag_generation, render_sample,
};

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
            tuiscotti::assert_screenshot!(bad, &screen, &ws.policy());
        }));
        let msg = panic_message(result.unwrap_err());
        assert!(msg.contains("invalid snapshot name"), "{bad:?}: {msg}");
    }
    // Nothing written: no fac_evil evidence, no escape above the evidence root.
    assert!(list_files(&ws.evidence).iter().all(|p| {
        !p.file_name()
            .unwrap()
            .to_string_lossy()
            .contains("fac_evil")
    }));
    let mut top: Vec<PathBuf> = list_files(ws.evidence.parent().unwrap());
    top.sort();
    assert_eq!(top, vec![ws.evidence.clone(), ws.snaps.clone()]);
}
