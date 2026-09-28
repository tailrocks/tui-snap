//! Canonical frame unit matrix: validation, diffing, digest, round trips.
//!
//! Covers: A→B text changes, style-only changes, cursor-only changes,
//! wide/continuation geometry, corrupt imports (each malformed class),
//! JSON round trips, deterministic digests, dimension-mismatch errors.

use tuisnap::{Cell, Color, Cursor, CursorStyle, Frame, Mods, Provenance, Rgb, UnderlineStyle};

fn prov() -> Provenance {
    Provenance {
        tool: "tuisnap".into(),
        tool_version: "test".into(),
        profile: "test".into(),
        source: "test".into(),
        argv: vec![],
        created_unix: 0,
    }
}

fn styled_frame(symbol: &str, mods: Mods, cursor: Cursor) -> Frame {
    let mut f = Frame::blank(4, 2, prov());
    let mut c = Cell::blank(0, 0);
    c.symbol = symbol.to_string();
    c.mods = mods;
    f.set(c);
    f.cursor = cursor;
    f
}

#[test]
fn text_change_detected() {
    let a = styled_frame("A", Mods::default(), Cursor::default());
    let b = styled_frame("B", Mods::default(), Cursor::default());
    let diffs = a.diff_cells(&b).unwrap();
    assert_eq!(diffs, vec![(0, 0)]);
    assert_ne!(a.digest(), b.digest());
}

#[test]
fn style_only_change_detected() {
    let a = styled_frame("A", Mods::default(), Cursor::default());
    let mods = Mods {
        bold: true,
        ..Default::default()
    };
    let b = styled_frame("A", mods, Cursor::default());
    // Same text, different style: text equal, cells differ, digest differs.
    assert_eq!(a.text(), b.text());
    assert_eq!(a.diff_cells(&b).unwrap(), vec![(0, 0)]);
    assert_ne!(a.digest(), b.digest());
}

#[test]
fn cursor_only_change_detected() {
    let a = styled_frame("A", Mods::default(), Cursor::default());
    let b = styled_frame(
        "A",
        Mods::default(),
        Cursor {
            x: 1,
            y: 0,
            visible: true,
            style: CursorStyle::Block,
            blinking: false,
        },
    );
    assert_eq!(a.text(), b.text());
    assert_eq!(a.diff_cells(&b).unwrap(), vec![(1, 0)]);
    assert_ne!(a.digest(), b.digest());
}

#[test]
fn identical_frames_match_and_digest_is_stable() {
    let a = styled_frame("A", Mods::default(), Cursor::default());
    let b = styled_frame("A", Mods::default(), Cursor::default());
    assert!(a.diff_cells(&b).unwrap().is_empty());
    assert_eq!(a.digest(), b.digest());
    assert_eq!(a.to_json_pretty(), b.to_json_pretty());
}

#[test]
fn dimension_mismatch_is_an_error_not_a_diff() {
    let a = Frame::blank(4, 2, prov());
    let b = Frame::blank(5, 2, prov());
    assert!(a.diff_cells(&b).is_err());
}

#[test]
fn json_round_trip_preserves_everything() {
    let mut f = styled_frame(
        "A",
        Mods {
            italic: true,
            underline: true,
            underline_style: UnderlineStyle::Single,
            ..Default::default()
        },
        Cursor {
            x: 2,
            y: 1,
            visible: true,
            style: CursorStyle::Bar,
            blinking: true,
        },
    );
    // Wide lead + continuation.
    let mut lead = Cell::blank(1, 0);
    lead.symbol = "日".to_string();
    lead.width = 2;
    f.set(lead);
    let mut cont = Cell::blank(2, 0);
    cont.symbol = String::new();
    cont.width = 0;
    cont.continuation = true;
    f.set(cont);
    f.validate().unwrap();
    let back = Frame::from_json(&f.to_json_pretty()).unwrap();
    assert_eq!(back.digest(), f.digest());
    assert_eq!(back.text(), f.text());
}

#[test]
fn corrupt_imports_rejected_explicitly() {
    let good = styled_frame("A", Mods::default(), Cursor::default());
    // Wrong version.
    // Wrong version.
    let mut bad = good.clone();
    bad.version = 99;
    assert!(bad.validate().is_err());
    // Zero dims.
    bad = good.clone();
    bad.cols = 0;
    assert!(bad.validate().is_err());
    // Cell count mismatch.
    bad = good.clone();
    bad.cells.pop();
    assert!(bad.validate().is_err());
    // Dangling continuation (cell (1,0) marked continuation without wide lead).
    bad = good.clone();
    let mut cont = Cell::blank(1, 0);
    cont.symbol = String::new();
    cont.width = 0;
    cont.continuation = true;
    bad.set(cont);
    assert!(bad.validate().is_err());
    // Wide cell overflowing its row.
    bad = good.clone();
    let mut wide = Cell::blank(3, 0);
    wide.symbol = "日".to_string();
    wide.width = 2;
    bad.set(wide);
    assert!(bad.validate().is_err());
    // Empty lead symbol.
    bad = good.clone();
    let mut empty = Cell::blank(0, 0);
    empty.symbol = String::new();
    bad.set(empty);
    assert!(bad.validate().is_err());
    // Visible cursor outside grid.
    bad = good.clone();
    bad.cursor = Cursor {
        x: 99,
        y: 0,
        visible: true,
        style: CursorStyle::Block,
        blinking: false,
    };
    assert!(bad.validate().is_err());
    // Malformed JSON and wrong-type JSON.
    assert!(Frame::from_json("{not json").is_err());
    assert!(Frame::from_json("[1,2,3]").is_err());
    // Valid frame passes.
    good.validate().unwrap();
}

#[test]
fn provenance_excluded_from_digest() {
    let mut a = styled_frame("A", Mods::default(), Cursor::default());
    let mut b = a.clone();
    b.provenance.created_unix = 9_999_999;
    b.provenance.argv = vec!["other".into()];
    assert_eq!(a.digest(), b.digest());
    let _ = &mut a;
}

#[test]
fn indexed_and_rgb_colors_resolve() {
    let (fg, bg) = Frame::resolve_cell(
        &Cell {
            fg: Color::Indexed(9),
            bg: Color::Rgb(Rgb::new(1, 2, 3)),
            ..Cell::blank(0, 0)
        },
        Rgb::new(0xd0, 0xd0, 0xd0),
        Rgb::new(0, 0, 0),
    );
    assert_eq!(fg, Rgb::from_indexed(9));
    assert_eq!(bg, Rgb::new(1, 2, 3));
    // Reverse swaps; dim blends toward bg.
    let rev = Cell {
        mods: Mods {
            reverse: true,
            ..Default::default()
        },
        ..Cell::blank(0, 0)
    };
    let (fg2, bg2) = Frame::resolve_cell(&rev, Rgb::new(10, 10, 10), Rgb::new(0, 0, 0));
    assert_eq!((fg2, bg2), (Rgb::new(0, 0, 0), Rgb::new(10, 10, 10)));
}
