//! Independent ANSI and buffer sentinels; expected values are not snapshots.
use tuiscotti::{Cell, Color, Frame, Profile, Provenance, Rgb, VENDORED_FACES};
fn prov() -> Provenance {
    Provenance::now("qualification", "fixture", vec![])
}

#[test]
fn hidden_svg_cells_keep_whitespace_geometry() {
    let mut frame = Frame::blank(4, 2, prov());
    for (i, symbol) in ["H", "A", "H", "B"].into_iter().enumerate() {
        frame.cells[i].symbol = symbol.into();
        frame.cells[i].mods.hidden = i % 2 == 0;
    }
    let svg = tuiscotti::render::render_svg(&frame, &Profile::default_profile());
    assert!(svg.contains("xml:space=\"preserve\""));
    assert!(svg.contains("> A B</text>"));
    assert!(!svg.contains('H'));
}

#[test]
fn dim_blending_uses_full_precision_before_narrowing() {
    for fg in 0..=255u8 {
        for bg in 0..=255u8 {
            let mut c = Cell::blank(0, 0);
            c.fg = Color::Rgb(Rgb::new(fg, fg, fg));
            c.bg = Color::Rgb(Rgb::new(bg, bg, bg));
            c.mods.dim = true;
            let (actual, _) = Frame::resolve_cell(&c, Rgb::new(0, 0, 0), Rgb::new(0, 0, 0));
            let expected = ((u16::from(fg) * 6 + u16::from(bg) * 4) / 10) as u8;
            assert_eq!(actual, Rgb::new(expected, expected, expected));
        }
    }
}

#[test]
fn hidden_and_blink_are_canonical_and_hidden_does_not_paint() {
    let mut hidden = Frame::blank(4, 2, prov());
    hidden.cells[0].symbol = "H".into();
    hidden.cells[0].mods.hidden = true;
    hidden.cells[0].mods.blink = true;
    let copy = Frame::from_json(&hidden.to_json()).unwrap();
    assert!(copy.cells[0].mods.hidden && copy.cells[0].mods.blink);
    let blank = Frame::blank(4, 2, prov());
    assert_ne!(blank.digest(), hidden.digest());
    let profile = Profile::default_profile();
    assert_eq!(
        tuiscotti::render::render_png(&hidden, &profile, &VENDORED_FACES).unwrap(),
        tuiscotti::render::render_png(&blank, &profile, &VENDORED_FACES).unwrap()
    );
    assert!(!tuiscotti::render::render_svg(&hidden, &profile).contains('H'));
}

#[test]
fn ratatui_preserves_combined_flags_and_wide_styles() {
    use ratatui::{
        buffer::Buffer,
        layout::Rect,
        style::{Color as C, Modifier, Style},
    };
    let mut b = Buffer::empty(Rect::new(0, 0, 8, 2));
    let style = Style::default()
        .fg(C::Rgb(1, 2, 3))
        .bg(C::Rgb(4, 5, 6))
        .add_modifier(Modifier::BOLD | Modifier::DIM | Modifier::HIDDEN | Modifier::SLOW_BLINK);
    b.set_string(0, 0, "界", style);
    let f = tuiscotti::ratatui::from_buffer(&b, 8, 2, None, prov());
    for x in [0, 1] {
        let c = f.get(x, 0).unwrap();
        assert_eq!(c.fg, Color::Rgb(Rgb::new(1, 2, 3)));
        assert_eq!(c.bg, Color::Rgb(Rgb::new(4, 5, 6)));
        assert!(c.mods.bold && c.mods.dim && c.mods.hidden && c.mods.blink);
    }
}

#[test]
fn schema_two_is_not_silently_reinterpreted() {
    let f = Frame::blank(2, 2, prov());
    assert!(Frame::from_json(&f.to_json().replace("\"version\":3", "\"version\":2")).is_err());
}
