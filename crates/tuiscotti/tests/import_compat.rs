//! Compat importer tests (backlog A10): asciinema cast, terminal-control
//! `.termctrl`, read-only/no-exec guarantees, frozen-tree conformance.

use std::fmt::Write as _;
use std::path::Path;
use tuiscotti::import_compat::{
    CompatError, ImportLimits, import_cast, import_cast_with, import_termctrl, import_termctrl_with,
};

fn write_tmp(
    dir: &Path,
    name: &str,
    bytes: &[u8],
) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    let p = dir.join(name);
    std::fs::write(&p, bytes)?;
    Ok(p)
}

// ---------------------------------------------------------------------------
// asciinema cast
// ---------------------------------------------------------------------------

#[test]
fn cast_roundtrip_with_export_writer() {
    let tmp = tempfile::tempdir().expect("tempfile::tempdir() succeeds");
    let frames = vec![
        ("hello\x1b[1m!\n".to_string(), 0.5),
        ("second line\n".to_string(), 1.25),
    ];
    let cast_path = tuiscotti::export::cast_v2(&frames, 80, 24, &tmp.path().join("out"))
        .expect("cast_v2 succeeds");
    let trace = import_cast(&cast_path).expect("import_cast succeeds");
    assert_eq!(trace.header.version, 2);
    assert_eq!((trace.header.width, trace.header.height), (80, 24));
    assert!(trace.unsupported.is_empty());
    let deltas = trace.output_deltas();
    assert_eq!(deltas.len(), 2);
    assert_eq!(
        String::from_utf8(deltas[0].1.clone()).expect("utf8 delta succeeds"),
        "hello\x1b[1m!\n"
    );
    assert_eq!(
        String::from_utf8(deltas[1].1.clone()).expect("utf8 delta succeeds"),
        "second line\n"
    );
    assert!((deltas[0].0 - 0.5).abs() < 1e-6, "dt0 = {}", deltas[0].0);
    assert!((deltas[1].0 - 1.25).abs() < 1e-6, "dt1 = {}", deltas[1].0);
}

#[test]
fn cast_input_never_fed_as_output() {
    let tmp = tempfile::tempdir().expect("tempfile::tempdir() succeeds");
    let doc = concat!(
        "{\"version\":2,\"width\":10,\"height\":5}\n",
        "[0.5,\"o\",\"out1\"]\n",
        "[0.6,\"i\",\"EVIL-INPUT\"]\n",
        "[0.7,\"m\",\"note\"]\n",
        "[1.0,\"o\",\"out2\"]\n",
    );
    let p = write_tmp(tmp.path(), "t.cast", doc.as_bytes()).expect("write_tmp succeeds");
    let trace = import_cast(&p).expect("import_cast succeeds");
    assert_eq!(trace.events.len(), 4);
    let deltas = trace.output_deltas();
    assert_eq!(deltas.len(), 2);
    assert!(!deltas.iter().any(|(_, b)| b == b"EVIL-INPUT"));
    // screens_via: caller replay sees output bytes only.
    let mut fed: Vec<u8> = Vec::new();
    let states = trace.screens_via(Vec::<u8>::new(), |mut acc: Vec<u8>, bytes: &[u8]| {
        fed.extend_from_slice(bytes);
        acc.extend_from_slice(bytes);
        acc
    });
    assert_eq!(states.len(), 2);
    assert_eq!(fed, b"out1out2");
    assert_eq!(states[1], b"out1out2");
}

#[test]
fn cast_bad_version_and_content_offsets() {
    let tmp = tempfile::tempdir().expect("tempfile::tempdir() succeeds");
    let p = write_tmp(
        tmp.path(),
        "v.cast",
        b"{\"version\":1,\"width\":80,\"height\":24}\n",
    )
    .expect("write_tmp succeeds");
    assert!(matches!(
        import_cast(&p),
        Err(CompatError::Version { offset: 0, .. })
    ));
    let p = write_tmp(tmp.path(), "empty.cast", b"").expect("write_tmp succeeds");
    assert!(matches!(
        import_cast(&p),
        Err(CompatError::Version { offset: 0, .. })
    ));
    let p = write_tmp(tmp.path(), "nojson.cast", b"nope\n").expect("write_tmp succeeds");
    assert!(matches!(
        import_cast(&p),
        Err(CompatError::Version { offset: 0, .. })
    ));
    // Bad event on line 3: offset must point at that line.
    let header = "{\"version\":2,\"width\":80,\"height\":24}\n";
    let good = "[0.5,\"o\",\"x\"]\n";
    let bad_line = "[\"bogus\"]\n";
    let doc = format!("{header}{good}{bad_line}");
    let p = write_tmp(tmp.path(), "bad.cast", doc.as_bytes()).expect("write_tmp succeeds");
    let want_off = u64::try_from(header.len() + good.len()).expect("offset fits u64");
    match import_cast(&p) {
        Err(CompatError::Content { offset, .. }) => assert_eq!(offset, want_off),
        r => panic!("want Content, got {r:?}"),
    }
}

#[test]
fn cast_unknown_code_is_unsupported_not_fatal() {
    let tmp = tempfile::tempdir().expect("tempfile::tempdir() succeeds");
    let doc = concat!(
        "{\"version\":2,\"width\":80,\"height\":24}\n",
        "[0.5,\"o\",\"x\"]\n",
        "[0.6,\"z\",\"future\"]\n",
    );
    let p = write_tmp(tmp.path(), "u.cast", doc.as_bytes()).expect("write_tmp succeeds");
    let trace = import_cast(&p).expect("import_cast succeeds");
    assert_eq!(trace.events.len(), 1);
    assert_eq!(trace.unsupported.len(), 1);
    assert!(
        trace.unsupported[0].contains("\"z\""),
        "{}",
        trace.unsupported[0]
    );
}

#[test]
fn cast_bounds_fail_never_truncate() {
    let tmp = tempfile::tempdir().expect("tempfile::tempdir() succeeds");
    let mut doc = String::from("{\"version\":2,\"width\":80,\"height\":24}\n");
    for i in 0..10 {
        writeln!(doc, "[{i}.0,\"o\",\"x\"]").expect("write to doc succeeds");
    }
    let p = write_tmp(tmp.path(), "b.cast", doc.as_bytes()).expect("write_tmp succeeds");
    let lim = ImportLimits {
        max_events: 5,
        ..Default::default()
    };
    assert!(matches!(
        import_cast_with(&p, &lim),
        Err(CompatError::TooLarge { what: "events", .. })
    ));
    let lim = ImportLimits {
        max_line_bytes: 4,
        ..Default::default()
    };
    assert!(matches!(
        import_cast_with(&p, &lim),
        Err(CompatError::TooLarge { what: "line", .. })
    ));
    let lim = ImportLimits {
        max_total_bytes: 10,
        ..Default::default()
    };
    assert!(matches!(
        import_cast_with(&p, &lim),
        Err(CompatError::TooLarge { what: "bytes", .. })
    ));
}

// ---------------------------------------------------------------------------
// terminal-control .termctrl
// ---------------------------------------------------------------------------

const TC_V2: &str = concat!(
    "{\"type\":\"header\",\"version\":2,\"cols\":80,\"rows\":24,\"cell_width\":9,\"cell_height\":18}\n",
    "{\"type\":\"output\",\"at_ms\":100,\"bytes\":[104,105]}\n",
    "{\"type\":\"input\",\"at_ms\":150,\"origin\":\"client\",\"bytes\":[113]}\n",
    "{\"type\":\"mouse\",\"at_ms\":160,\"event\":{\"action\":\"click\",\"x\":1,\"y\":2},\"bytes\":[27,91,77]}\n",
    "{\"type\":\"resize\",\"at_ms\":200,\"cols\":100,\"rows\":30,\"cell_width\":9,\"cell_height\":18}\n",
    "{\"type\":\"marker\",\"at_ms\":250,\"name\":\"checkpoint\"}\n",
    "{\"type\":\"output\",\"at_ms\":300,\"bytes\":[33]}\n",
);

#[test]
fn termctrl_v2_output_only_and_loss_clean() {
    let tmp = tempfile::tempdir().expect("tempfile::tempdir() succeeds");
    let p = write_tmp(tmp.path(), "r.termctrl", TC_V2.as_bytes()).expect("write_tmp succeeds");
    let trace = import_termctrl(&p).expect("import_termctrl succeeds");
    assert_eq!(trace.version, 2);
    assert_eq!((trace.cols, trace.rows), (80, 24));
    assert_eq!(trace.events.len(), 6);
    assert!(trace.loss.is_clean());
    let deltas = trace.output_deltas();
    assert_eq!(deltas.len(), 2);
    assert_eq!(deltas[0].1, b"hi");
    assert!((deltas[0].0 - 0.1).abs() < 1e-9);
    assert_eq!(deltas[1].1, b"!");
    assert!((deltas[1].0 - 0.2).abs() < 1e-9);
    // screens_via never sees input/mouse bytes (q, ESC [ M).
    let states = trace.screens_via(Vec::<u8>::new(), |mut acc: Vec<u8>, bytes: &[u8]| {
        acc.extend_from_slice(bytes);
        acc
    });
    assert_eq!(states.len(), 2);
    assert_eq!(states[1], b"hi!");
}

#[test]
fn termctrl_v1_mouse_dropped_and_reported() {
    let tmp = tempfile::tempdir().expect("tempfile::tempdir() succeeds");
    let doc = concat!(
        "{\"type\":\"header\",\"version\":1,\"cols\":80,\"rows\":24,\"cell_width\":9,\"cell_height\":18}\n",
        "{\"type\":\"mouse\",\"at_ms\":10,\"event\":{\"action\":\"click\",\"x\":1,\"y\":2},\"bytes\":[1]}\n",
        "{\"type\":\"output\",\"at_ms\":20,\"bytes\":[65]}\n",
    );
    let p = write_tmp(tmp.path(), "v1.termctrl", doc.as_bytes()).expect("write_tmp succeeds");
    let trace = import_termctrl(&p).expect("import_termctrl succeeds");
    assert_eq!(trace.version, 1);
    assert_eq!(trace.events.len(), 1);
    assert_eq!(trace.loss.dropped_events.len(), 1);
    assert!(trace.loss.dropped_events[0].contains("version 2"));
    assert!(trace.loss.unsupported_fields.is_empty());
}

#[test]
fn termctrl_unknown_field_and_type_reported() {
    let tmp = tempfile::tempdir().expect("tempfile::tempdir() succeeds");
    let doc = concat!(
        "{\"type\":\"header\",\"version\":2,\"cols\":80,\"rows\":24,\"cell_width\":9,\"cell_height\":18}\n",
        "{\"type\":\"output\",\"at_ms\":10,\"bytes\":[65],\"future_flag\":true}\n",
        "{\"type\":\"teleport\",\"at_ms\":20}\n",
    );
    let p = write_tmp(tmp.path(), "w.termctrl", doc.as_bytes()).expect("write_tmp succeeds");
    let trace = import_termctrl(&p).expect("import_termctrl succeeds");
    assert_eq!(trace.events.len(), 1);
    assert_eq!(trace.loss.unsupported_fields.len(), 1);
    assert!(trace.loss.unsupported_fields[0].contains("output.future_flag"));
    assert_eq!(trace.loss.dropped_events.len(), 1);
    assert!(trace.loss.dropped_events[0].contains("teleport"));
}

#[test]
fn termctrl_version_and_content_errors() {
    let tmp = tempfile::tempdir().expect("tempfile::tempdir() succeeds");
    let p = write_tmp(
        tmp.path(),
        "v9.termctrl",
        b"{\"type\":\"header\",\"version\":9,\"cols\":80,\"rows\":24}\n",
    )
    .expect("write_tmp succeeds");
    assert!(matches!(
        import_termctrl(&p),
        Err(CompatError::Version { offset: 0, .. })
    ));
    let p = write_tmp(
        tmp.path(),
        "noh.termctrl",
        b"{\"type\":\"output\",\"at_ms\":1,\"bytes\":[1]}\n",
    )
    .expect("write_tmp succeeds");
    assert!(matches!(
        import_termctrl(&p),
        Err(CompatError::Version { offset: 0, .. })
    ));
    let header = "{\"type\":\"header\",\"version\":1,\"cols\":80,\"rows\":24,\"cell_width\":9,\"cell_height\":18}\n";
    let bad_line = "{\"type\":\"output\",\"at_ms\":\"soon\",\"bytes\":[1]}\n";
    let doc = format!("{header}{bad_line}");
    let p = write_tmp(tmp.path(), "bad.termctrl", doc.as_bytes()).expect("write_tmp succeeds");
    match import_termctrl(&p) {
        Err(CompatError::Content { offset, .. }) => {
            assert_eq!(
                offset,
                u64::try_from(header.len()).expect("offset fits u64")
            );
        }
        r => panic!("want Content, got {r:?}"),
    }
    // Byte out of range.
    let doc = format!("{header}{{\"type\":\"output\",\"at_ms\":1,\"bytes\":[999]}}\n");
    let p = write_tmp(tmp.path(), "bigr.termctrl", doc.as_bytes()).expect("write_tmp succeeds");
    assert!(matches!(
        import_termctrl(&p),
        Err(CompatError::Content { .. })
    ));
}

#[test]
fn termctrl_bounds_fail() {
    let tmp = tempfile::tempdir().expect("tempfile::tempdir() succeeds");
    let p = write_tmp(tmp.path(), "b.termctrl", TC_V2.as_bytes()).expect("write_tmp succeeds");
    let lim = ImportLimits {
        max_events: 2,
        ..Default::default()
    };
    assert!(matches!(
        import_termctrl_with(&p, &lim),
        Err(CompatError::TooLarge { what: "events", .. })
    ));
}

// ---------------------------------------------------------------------------
// Read-only + never-execute (canary)
// ---------------------------------------------------------------------------

#[test]
fn imports_never_write_nor_execute_canary() {
    let tmp = tempfile::tempdir().expect("tempfile::tempdir() succeeds");
    let canary = tmp.path().join("CANARY-SPAWNED");
    let cmd = format!("touch {}", canary.display());
    // Embed the "command" everywhere an importer might be tempted to run it.
    let cast = format!(
        "{{\"version\":2,\"width\":80,\"height\":24,\"title\":{cmd:?}}}\n[0.5,\"o\",\"x\"]\n[0.6,\"m\",{cmd:?}]\n"
    );
    let cast_path =
        write_tmp(tmp.path(), "evil.cast", cast.as_bytes()).expect("write_tmp succeeds");
    let tc = format!(
        "{{\"type\":\"header\",\"version\":2,\"cols\":80,\"rows\":24,\"cell_width\":9,\"cell_height\":18}}\n\
         {{\"type\":\"marker\",\"at_ms\":1,\"name\":{cmd:?}}}\n\
         {{\"type\":\"input\",\"at_ms\":2,\"origin\":\"host\",\"bytes\":[105]}}\n\
         {{\"type\":\"output\",\"at_ms\":3,\"bytes\":[111]}}\n"
    );
    let tc_path =
        write_tmp(tmp.path(), "evil.termctrl", tc.as_bytes()).expect("write_tmp succeeds");
    let cast_before = std::fs::read(&cast_path).expect("fs::read succeeds");
    let tc_before = std::fs::read(&tc_path).expect("fs::read succeeds");
    let before_entries: Vec<_> = std::fs::read_dir(tmp.path())
        .expect("read_dir succeeds")
        .map(|e| e.expect("dir entry succeeds").file_name())
        .collect();

    let ctrace = import_cast(&cast_path).expect("import_cast succeeds");
    let ttrace = import_termctrl(&tc_path).expect("import_termctrl succeeds");
    // Even replay helpers only move bytes through caller code.
    let _ = ctrace.screens_via((), |(), _| ());
    let _ = ttrace.screens_via((), |(), _| ());

    assert!(!canary.exists(), "importer spawned the recorded command");
    assert_eq!(
        std::fs::read(&cast_path).expect("fs::read succeeds"),
        cast_before
    );
    assert_eq!(
        std::fs::read(&tc_path).expect("fs::read succeeds"),
        tc_before
    );
    let after_entries: Vec<_> = std::fs::read_dir(tmp.path())
        .expect("read_dir succeeds")
        .map(|e| e.expect("dir entry succeeds").file_name())
        .collect();
    assert_eq!(before_entries.len(), after_entries.len());
    // No sibling files appeared beside the sources.
    assert_eq!(after_entries.len(), 2);
}

// ---------------------------------------------------------------------------
// Own-store frozen conformance
// ---------------------------------------------------------------------------

#[test]
fn frozen_four_file_tree_roundtrip_readonly() {
    use tuiscotti::assert::{emit_four, import_frozen_v1};
    let tmp = tempfile::tempdir().expect("tempfile::tempdir() succeeds");
    let dir = tmp.path().join("frozen");
    let screen = tuiscotti::Screen::blank(20, 6);
    let emitted = emit_four(&screen, &dir).expect("emit_four succeeds");
    for p in [&emitted.ansi, &emitted.txt, &emitted.png, &emitted.html] {
        assert!(p.exists(), "missing {}", p.display());
    }
    let before: Vec<(std::path::PathBuf, Vec<u8>)> =
        [&emitted.ansi, &emitted.txt, &emitted.png, &emitted.html]
            .into_iter()
            .map(|p| (p.clone(), std::fs::read(p).expect("fs::read succeeds")))
            .collect();

    let tree = import_frozen_v1(&dir).expect("import_frozen succeeds");
    assert_eq!(tree.scenarios.len(), 1);
    assert_eq!(tree.scenarios[0].name, "snapshot");
    assert!(tree.unsupported.is_empty(), "{:?}", tree.unsupported);
    assert!(!tree.scenarios[0].ansi.is_empty());
    assert!(!tree.scenarios[0].txt.is_empty());
    assert!(!tree.scenarios[0].png.is_empty());

    // Approved tree untouched by the read-only import.
    for (p, bytes) in &before {
        assert_eq!(
            &std::fs::read(p).expect("fs::read succeeds"),
            bytes,
            "{}",
            p.display()
        );
    }
}
