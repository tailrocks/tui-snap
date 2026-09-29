//! Underline style + color (M03): SGR 4:x / 58 parse → model → canonical → assert.
//!
//! Additive v3 change: [`Mods::underline`] (bool) is untouched, and the new
//! [`Mods::underline_style`] / [`Cell::underline_color`] fields default +
//! skip-serialize, so legacy files parse and only new data carries the keys.
//! Readers use [`Mods::effective_underline_style`] so legacy `underline=true`
//! cells and new styled cells agree.
//!
//! The emulator (vte/alacritty) already parses `4`, `4:0..4:5`, `24`, `58…`,
//! `59`; these tests pin that the replay adapters surface the style flags +
//! underline color, that the ratatui adapter maps what upstream exposes
//! (UNDERLINED bit → Single, `underline_color` field), that canonical
//! projections carry the state sparsely (absent when default), and that the
//! assert helper accepts exact matches and rejects style/color drift.

use tuiscotti::assert::{assert_underline_at, check_underline_at};
use tuiscotti::frame::{Cell, Color, Mods, Rgb, UnderlineStyle};
use tuiscotti::insta_proto::{insta_string, insta_value};
use tuiscotti::screen::Screen;

/// Set underline state the way producers do: bool and style consistent.
fn set_ul(cell: &mut Cell, style: UnderlineStyle, color: Color) {
    cell.mods.underline = style.is_some();
    cell.mods.underline_style = style;
    cell.underline_color = color;
}

fn screen_from(mut cells: Vec<Cell>, cols: u16, rows: u16) -> Screen {
    for (i, c) in cells.iter_mut().enumerate() {
        c.x = (i % cols as usize) as u16;
        c.y = (i / cols as usize) as u16;
    }
    Screen::validate(cols, rows, 0, 0, cells, tuiscotti::frame::Cursor::default()).unwrap()
}

fn blank_row(cols: u16) -> Vec<Cell> {
    (0..cols).map(|x| Cell::blank(x, 0)).collect()
}

// ---------------------------------------------------------------------------
// ANSI replay: SGR 4:x styles (pty)
// ---------------------------------------------------------------------------

#[cfg(feature = "pty")]
#[test]
fn sgr_underline_styles_parse_to_model() {
    use tuiscotti::tui_shell::replay_bytes;
    // 4:2 double, 4:3 curly, 4:4 dotted, 4:5 dashed, 4 single, 24 cancel.
    let out = b"\x1b[4:2mA\x1b[4:3mB\x1b[4:4mC\x1b[4:5mD\x1b[4mE\x1b[24mF";
    let replayed = replay_bytes(out, 6, 1).unwrap();
    let got: Vec<UnderlineStyle> = (0..6)
        .map(|x| {
            replayed
                .screen
                .get(x, 0)
                .unwrap()
                .mods
                .effective_underline_style()
        })
        .collect();
    assert_eq!(
        got,
        vec![
            UnderlineStyle::Double,
            UnderlineStyle::Curly,
            UnderlineStyle::Dotted,
            UnderlineStyle::Dashed,
            UnderlineStyle::Single,
            UnderlineStyle::None,
        ]
    );
    // Producer invariant: bool agrees with the style on every cell.
    for x in 0..6 {
        let m = replayed.screen.get(x, 0).unwrap().mods;
        assert_eq!(m.underline, m.underline_style.is_some(), "cell {x}");
    }
}

#[cfg(feature = "pty")]
#[test]
fn sgr_underline_color_forms_parse_to_model() {
    use tuiscotti::tui_shell::replay_bytes;
    let out = b"\x1b[58;5;9mA\x1b[58;2;1;2;3mB\x1b[58:5:4mC\x1b[59mD";
    let replayed = replay_bytes(out, 4, 1).unwrap();
    let got: Vec<Color> = (0..4)
        .map(|x| replayed.screen.get(x, 0).unwrap().underline_color)
        .collect();
    assert_eq!(
        got,
        vec![
            Color::Indexed(9),
            Color::Rgb(Rgb::new(1, 2, 3)),
            Color::Indexed(4),
            Color::Default,
        ]
    );
}

#[cfg(feature = "pty")]
#[test]
fn sgr_cancel_and_reset_clear_style_and_color() {
    use tuiscotti::tui_shell::replay_bytes;
    // 4:0 cancels the style but keeps the color; SGR 0 clears both.
    let replayed = replay_bytes(b"\x1b[4:2m\x1b[58;5;9mA\x1b[4:0mB\x1b[0mC", 3, 1).unwrap();
    let s = &replayed.screen;
    let m0 = s.get(0, 0).unwrap().mods;
    assert_eq!(m0.effective_underline_style(), UnderlineStyle::Double);
    assert!(m0.underline);
    assert_eq!(s.get(0, 0).unwrap().underline_color, Color::Indexed(9));
    let m1 = s.get(1, 0).unwrap().mods;
    assert_eq!(m1.effective_underline_style(), UnderlineStyle::None);
    assert!(!m1.underline);
    assert_eq!(s.get(1, 0).unwrap().underline_color, Color::Indexed(9));
    let m2 = s.get(2, 0).unwrap().mods;
    assert_eq!(m2.effective_underline_style(), UnderlineStyle::None);
    assert_eq!(s.get(2, 0).unwrap().underline_color, Color::Default);
}

// ---------------------------------------------------------------------------
// Ratatui adapter: UNDERLINED → Single, underline_color mapped
// ---------------------------------------------------------------------------

#[test]
fn ratatui_underlined_maps_to_single_with_color() {
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::style::{Color as RColor, Modifier, Style};
    use tuiscotti::ratatui::{EdgePolicy, screen_from_buffer};

    let mut buf = Buffer::empty(Rect::new(0, 0, 2, 1));
    buf.cell_mut((0, 0)).unwrap().set_symbol("A").set_style(
        Style::default()
            .add_modifier(Modifier::UNDERLINED)
            .underline_color(RColor::Red),
    );
    buf.cell_mut((1, 0)).unwrap().set_symbol("B");
    let cap = screen_from_buffer(&buf, None, EdgePolicy::default()).unwrap();
    let a = cap.screen.get(0, 0).unwrap();
    assert!(a.mods.underline);
    assert_eq!(a.mods.effective_underline_style(), UnderlineStyle::Single);
    assert_eq!(a.underline_color, Color::Indexed(1));
    let b = cap.screen.get(1, 0).unwrap();
    assert!(!b.mods.underline);
    assert_eq!(b.mods.effective_underline_style(), UnderlineStyle::None);
    assert_eq!(b.underline_color, Color::Default);
}

// ---------------------------------------------------------------------------
// Canonical projections: styled state present, defaults absent
// ---------------------------------------------------------------------------

#[test]
fn canonical_text_carries_style_and_color_sparsely() {
    let mut cells = blank_row(2);
    cells[0].symbol = "A".to_string();
    set_ul(&mut cells[0], UnderlineStyle::Double, Color::Indexed(9));
    let text = insta_string(&screen_from(cells, 2, 1));
    assert!(
        text.contains("mods=double-underline uc=index=9"),
        "styled cell must carry style + color:\n{text}"
    );
    // Default cell line keeps the exact old shape (no uc= suffix).
    assert!(
        text.contains("cell 1,0 sym=\" \" w=1 cont=false fg=default bg=default mods=-\n"),
        "default cell line must be unchanged:\n{text}"
    );

    let plain = insta_string(&screen_from(blank_row(2), 2, 1));
    assert!(
        !plain.contains("uc="),
        "no color keys when default:\n{plain}"
    );
    assert!(
        !plain.contains("underline"),
        "no style tokens when default:\n{plain}"
    );
}

#[test]
fn canonical_json_carries_style_and_color_sparsely() {
    let mut cells = blank_row(2);
    cells[0].symbol = "A".to_string();
    set_ul(
        &mut cells[0],
        UnderlineStyle::Curly,
        Color::Rgb(Rgb::new(1, 2, 3)),
    );
    let v = insta_value(&screen_from(cells, 2, 1));
    assert_eq!(v["cells"][0]["mods"]["underline"], serde_json::json!(true));
    assert_eq!(
        v["cells"][0]["mods"]["underline_style"],
        serde_json::json!("undercurl")
    );
    assert_eq!(
        v["cells"][0]["underline_color"],
        serde_json::json!("#010203")
    );
    // Default cell: bool stays false, sparse keys absent.
    assert_eq!(v["cells"][1]["mods"]["underline"], serde_json::json!(false));
    assert!(v["cells"][1]["mods"].get("underline_style").is_none());
    assert!(v["cells"][1].get("underline_color").is_none());
}

// ---------------------------------------------------------------------------
// Assert helper: exact match passes, drift fails loud
// ---------------------------------------------------------------------------

#[test]
fn assert_helper_accepts_exact_match() {
    let mut cells = blank_row(1);
    set_ul(&mut cells[0], UnderlineStyle::Dotted, Color::Indexed(4));
    let screen = screen_from(cells, 1, 1);
    assert_underline_at(&screen, 0, 0, UnderlineStyle::Dotted, Color::Indexed(4));
}

#[test]
fn assert_helper_rejects_style_drift() {
    let mut cells = blank_row(1);
    set_ul(&mut cells[0], UnderlineStyle::Single, Color::Default);
    let screen = screen_from(cells, 1, 1);
    let err =
        check_underline_at(&screen, 0, 0, UnderlineStyle::Double, Color::Default).unwrap_err();
    assert!(err.contains("Double"), "must name want: {err}");
    assert!(err.contains("Single"), "must name got: {err}");
}

#[test]
fn assert_helper_rejects_color_drift_and_missing_cells() {
    let mut cells = blank_row(1);
    set_ul(&mut cells[0], UnderlineStyle::Single, Color::Indexed(1));
    let screen = screen_from(cells, 1, 1);
    let err =
        check_underline_at(&screen, 0, 0, UnderlineStyle::Single, Color::Indexed(2)).unwrap_err();
    assert!(
        err.contains("Indexed(2)") && err.contains("Indexed(1)"),
        "{err}"
    );
    assert!(check_underline_at(&screen, 9, 9, UnderlineStyle::None, Color::Default).is_err());
}

// ---------------------------------------------------------------------------
// Locator predicates + diff/digest + JSON sparsity
// ---------------------------------------------------------------------------

#[test]
fn style_query_matches_style_and_color() {
    use tuiscotti::locate::{Locator, StyleQuery};
    let mut cells = blank_row(3);
    set_ul(&mut cells[0], UnderlineStyle::Dashed, Color::Indexed(9));
    set_ul(&mut cells[1], UnderlineStyle::Single, Color::Default);
    let screen = screen_from(cells, 3, 1);
    let spans = Locator::style(
        StyleQuery::new()
            .underline_style(UnderlineStyle::Dashed)
            .underline_color(Color::Indexed(9)),
    )
    .resolve(&screen, 0)
    .unwrap();
    assert_eq!(spans.len(), 1);
    assert_eq!((spans[0].x, spans[0].end_x), (0, 1));
    // Coarse bool still works (negative: no underline on cell 2).
    let spans = Locator::style(StyleQuery::new().underline(false))
        .resolve(&screen, 0)
        .unwrap();
    assert_eq!(spans.len(), 1);
    assert_eq!((spans[0].x, spans[0].end_x), (2, 3));
}

#[test]
fn diff_and_digest_see_style_and_color() {
    use tuiscotti::frame::{Frame, Provenance};
    let prov = || Provenance {
        tool: "test".into(),
        tool_version: "test".into(),
        profile: "test".into(),
        source: "test".into(),
        argv: vec![],
        created_unix: 0,
    };
    let a = Frame::blank(2, 1, prov());
    let mut b = a.clone();
    b.cells[0].mods.underline = true;
    b.cells[0].mods.underline_style = UnderlineStyle::Double;
    assert_eq!(a.diff_cells(&b).unwrap(), vec![(0, 0)]);
    assert_ne!(a.digest(), b.digest());
    let mut c = a.clone();
    c.cells[1].underline_color = Color::Indexed(5);
    assert_eq!(a.diff_cells(&c).unwrap(), vec![(1, 0)]);
    assert_ne!(a.digest(), c.digest());
}

#[test]
fn frame_json_omits_new_keys_when_default_and_round_trips_styled() {
    use tuiscotti::frame::{Frame, Provenance};
    let prov = Provenance {
        tool: "test".into(),
        tool_version: "test".into(),
        profile: "test".into(),
        source: "test".into(),
        argv: vec![],
        created_unix: 0,
    };
    let plain = Frame::blank(1, 1, prov.clone());
    let json = plain.to_json();
    assert!(json.contains("\"version\":3"), "{json}");
    assert!(
        !json.contains("underline_style") && !json.contains("underline_color"),
        "new keys must stay sparse: {json}"
    );
    let mut styled = Frame::blank(1, 1, prov);
    set_ul(
        &mut styled.cells[0],
        UnderlineStyle::Curly,
        Color::Indexed(9),
    );
    let back = Frame::from_json(&styled.to_json()).unwrap();
    assert_eq!(
        back.cells[0].mods.effective_underline_style(),
        UnderlineStyle::Curly
    );
    assert_eq!(back.cells[0].underline_color, Color::Indexed(9));
}

#[test]
fn legacy_v3_bool_only_json_reads_as_single() {
    use tuiscotti::frame::Frame;
    // Hand-written v3 shape: bool present, new keys absent.
    let json = r#"{"version":3,"cols":1,"rows":1,"cells":[{"x":0,"y":0,"symbol":"A","width":1,"continuation":false,"fg":"Default","bg":"Default","mods":{"hidden":false,"blink":false,"bold":false,"dim":false,"italic":false,"underline":true,"strikethrough":false,"reverse":false}}],"cursor":{"x":0,"y":0,"visible":false,"style":"Block","blinking":false},"provenance":{"tool":"t","tool_version":"t","profile":"t","source":"t","argv":[],"created_unix":0}}"#;
    let frame = Frame::from_json(json).unwrap();
    assert!(frame.cells[0].mods.underline);
    // Import normalizes legacy bool-only cells to the producer form.
    assert_eq!(frame.cells[0].mods.underline_style, UnderlineStyle::Single);
    // A hand-built legacy-shape Mods still reads Single via effective style.
    let mods = Mods {
        underline: true,
        ..Mods::default()
    };
    assert_eq!(mods.effective_underline_style(), UnderlineStyle::Single);
}

#[test]
fn all_approved_frames_verify_unchanged() {
    use tuiscotti::frame::Frame;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/visual/approved");
    let mut count = 0;
    let mut entries: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().ends_with(".frame.json"))
        })
        .collect();
    entries.sort();
    for path in entries {
        let text = std::fs::read_to_string(&path).unwrap();
        Frame::from_json(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        count += 1;
    }
    assert_eq!(count, 24, "expected all 24 approved frames");
}

#[test]
fn ansi_dump_emits_style_and_color() {
    use tuiscotti::frame::{Frame, Provenance};
    let mut f = Frame::blank(
        2,
        1,
        Provenance {
            tool: "test".into(),
            tool_version: "test".into(),
            profile: "test".into(),
            source: "test".into(),
            argv: vec![],
            created_unix: 0,
        },
    );
    f.cells[0].symbol = "A".to_string();
    set_ul(&mut f.cells[0], UnderlineStyle::Double, Color::Indexed(9));
    let dump = tuiscotti::render::ansi_dump(&f);
    assert!(
        dump.contains("4:2"),
        "double style must round-trip: {dump:?}"
    );
    assert!(dump.contains("58;5;9"), "ucolor must round-trip: {dump:?}");
}
