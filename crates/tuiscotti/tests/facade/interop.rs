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
    let screen = fixture().expect("fixture succeeds");
    let a = tempfile::Builder::new()
        .prefix("facade-emit-a-")
        .tempdir()
        .expect("tempfile::Builder::new() .prefix(\"facade-emit-a-\") .tempdir() succeeds");
    let b = tempfile::Builder::new()
        .prefix("facade-emit-b-")
        .tempdir()
        .expect("tempfile::Builder::new() .prefix(\"facade-emit-b-\") .tempdir() succeeds");
    let pa = emit_four(&screen, a.path()).expect("emit_four(&screen, a.path()) succeeds");
    let pb = emit_four(&screen, b.path()).expect("emit_four(&screen, b.path()) succeeds");
    for (fa, fb) in [
        (pa.ansi, pb.ansi),
        (pa.txt, pb.txt),
        (pa.png, pb.png),
        (pa.html, pb.html),
    ] {
        assert_eq!(
            fs::read(&fa).expect("fs::read(&fa) succeeds"),
            fs::read(&fb).expect("fs::read(&fb) succeeds"),
            "{fa:?}"
        );
    }
}

#[test]
fn import_frozen_v1_roundtrip_and_unsupported() {
    let screen = fixture().expect("fixture succeeds");
    let dir = tempfile::Builder::new()
        .prefix("facade-import-")
        .tempdir()
        .expect("tempfile::Builder::new() .prefix(\"facade-import-\") .tempdir() succeeds");
    emit_four(&screen, dir.path()).expect("emit_four(&screen, dir.path()) succeeds");
    let tree = import_frozen_v1(dir.path()).expect("import_frozen_v1(dir.path()) succeeds");
    assert_eq!(tree.scenarios.len(), 1);
    assert_eq!(tree.scenarios[0].name, "snapshot");
    assert!(tree.unsupported.is_empty(), "{:?}", tree.unsupported);
    assert_eq!(
        tree.scenarios[0].png,
        fs::read(dir.path().join("snapshot.png")).expect("fs::read snapshot.png succeeds")
    );
    assert_eq!(
        tree.scenarios[0].html,
        fs::read_to_string(dir.path().join("snapshot.html"))
            .expect("fs::read_to_string snapshot.html succeeds")
    );
    // Extra file + unknown embedded field are REPORTED, not fatal.
    fs::write(dir.path().join("notes.md"), "reviewer notes").expect("fs::write notes.md succeeds");
    let html = fs::read_to_string(dir.path().join("snapshot.html"))
        .expect("fs::read_to_string snapshot.html succeeds");
    let doctored = html.replacen("{\"version\":", "{\"zzz\":1,\"version\":", 1);
    assert_ne!(doctored, html, "setup: embed must be rewritten");
    fs::write(dir.path().join("snapshot.html"), doctored)
        .expect("fs::write snapshot.html succeeds");
    let tree = import_frozen_v1(dir.path()).expect("import_frozen_v1(dir.path()) succeeds");
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
    let screen = fixture().expect("fixture succeeds");
    // Incomplete stem: missing .txt.
    let dir = tempfile::Builder::new()
        .prefix("facade-bad-")
        .tempdir()
        .expect("tempfile::Builder::new() .prefix(\"facade-bad-\") .tempdir() succeeds");
    emit_four(&screen, dir.path()).expect("emit_four(&screen, dir.path()) succeeds");
    fs::remove_file(dir.path().join("snapshot.txt"))
        .expect("fs::remove_file(dir.path().join(\"snapshot.txt\")) succeeds");
    let err = import_frozen_v1(dir.path()).expect_err("import_frozen_v1(dir.path()) is an error");
    assert!(matches!(err, ImportError::Incomplete { .. }), "{err}");
    // Invalid stem name (backslash is a legal unix filename char, illegal here).
    let dir2 = tempfile::Builder::new()
        .prefix("facade-bad2-")
        .tempdir()
        .expect("tempfile::Builder::new() .prefix(\"facade-bad2-\") .tempdir() succeeds");
    for ext in ["ansi", "txt", "png", "html"] {
        fs::write(dir2.path().join(format!("we\\ird.{ext}")), b"x")
            .expect("fs::write(dir2.path().join(format!(\"we\\\\ird.{ext}\")), b\"x\") succeeds");
    }
    let err = import_frozen_v1(dir2.path()).expect_err("import_frozen_v1(dir2.path()) is an error");
    assert!(matches!(err, ImportError::InvalidName(_)), "{err}");
    // Undecodable PNG.
    let dir3 = tempfile::Builder::new()
        .prefix("facade-bad3-")
        .tempdir()
        .expect("tempfile::Builder::new() .prefix(\"facade-bad3-\") .tempdir() succeeds");
    emit_four(&screen, dir3.path()).expect("emit_four(&screen, dir3.path()) succeeds");
    fs::write(dir3.path().join("snapshot.png"), b"junk")
        .expect("fs::write(dir3.path().join(\"snapshot.png\"), b\"junk\") succeeds");
    let err = import_frozen_v1(dir3.path()).expect_err("import_frozen_v1(dir3.path()) is an error");
    assert!(matches!(err, ImportError::Corrupt { .. }), "{err}");
}

// ---------------------------------------------------------------------------
// Generation binding unit checks
// ---------------------------------------------------------------------------
#[test]
fn generation_binding_roundtrip() {
    assert_eq!(generation_id("x"), generation_id("x"));
    assert_ne!(generation_id("x"), generation_id("y"));
    let sample =
        render_sample(&fixture().expect("fixture succeeds")).expect("render_sample succeeds");
    let tagged = png_tag_generation(&sample.png, "g1");
    assert_eq!(png_generation(&tagged).as_deref(), Some("g1"));
    assert_eq!(png_generation(&sample.png), None);
    assert_eq!(png_generation(b"junk"), None);
    // Strict gate fails on missing bindings.
    let empty = tempfile::Builder::new()
        .prefix("facade-empty-")
        .tempdir()
        .expect("tempfile::Builder::new() .prefix(\"facade-empty-\") .tempdir() succeeds");
    assert!(check_consistent(empty.path(), "c", "p").is_err());
}

// ---------------------------------------------------------------------------
// F2: evidence names are validated before any write
// ---------------------------------------------------------------------------
#[test]
fn screenshot_rejects_unsafe_names_before_evidence() {
    let ws = workspace().expect("workspace succeeds");
    let screen = fixture().expect("fixture succeeds");
    for bad in [
        "../../fac_evil",
        "/tmp/fac_evil_abs",
        "fac_nest/../fac_evil_dotdot",
    ] {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            tuiscotti::assert_screenshot!(bad, &screen, &ws.policy());
        }));
        let msg = panic_message(&*result.expect_err("result is an error"));
        assert!(msg.contains("invalid snapshot name"), "{bad:?}: {msg}");
    }
    // Nothing written: no fac_evil evidence, no escape above the evidence root.
    assert!(
        list_files(&ws.evidence)
            .expect("list_files succeeds")
            .iter()
            .all(|p| {
                !p.file_name()
                    .expect("p.file_name() is some")
                    .to_string_lossy()
                    .contains("fac_evil")
            })
    );
    let mut top: Vec<PathBuf> =
        list_files(ws.evidence.parent().expect("ws.evidence.parent() is some"))
            .expect("list_files succeeds");
    top.sort();
    assert_eq!(top, vec![ws.evidence.clone(), ws.snaps.clone()]);
}
