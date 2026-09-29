use super::*;

#[test]
fn region_crop_preserves_geometry_and_origin() {
    let parent = Screen::validate(6, 4, 10, 20, blank_cells(6, 4), Cursor::default())
        .expect("Screen::validate(6, 4, 10, 20, blank_cells(6, 4), Cursor::default()) succeeds");
    let r = parent
        .region(2, 1, 3, 2, RegionPolicy::Clip)
        .expect("parent.region(2, 1, 3, 2, RegionPolicy::Clip) succeeds");
    assert_eq!(r.policy(), RegionPolicy::Clip);
    let s = r.screen();
    assert_eq!((s.cols(), s.rows()), (3, 2));
    assert_eq!(s.origin(), (12, 21));
    assert_eq!(s.get(0, 0).expect("s.get(0, 0) is some").x, 0);
    assert_eq!(s.get(2, 1).expect("s.get(2, 1) is some").y, 1);
}

#[test]
fn region_crop_translates_cursor_inside_and_hides_outside() {
    let mut cells = blank_cells(4, 2);
    let _ = &mut cells;
    let cursor = Cursor {
        x: 3,
        y: 0,
        visible: true,
        style: CursorStyle::Bar,
        blinking: true,
    };
    let parent = Screen::validate(4, 2, 0, 0, blank_cells(4, 2), cursor)
        .expect("Screen::validate(4, 2, 0, 0, blank_cells(4, 2), cursor) succeeds");
    let inside = parent
        .region(2, 0, 2, 1, RegionPolicy::Clip)
        .expect("parent.region(2, 0, 2, 1, RegionPolicy::Clip) succeeds");
    let c = inside.screen().cursor();
    assert!(c.visible);
    assert_eq!((c.x, c.y), (1, 0));
    let outside = parent
        .region(0, 1, 2, 1, RegionPolicy::Clip)
        .expect("parent.region(0, 1, 2, 1, RegionPolicy::Clip) succeeds");
    assert!(!outside.screen().cursor().visible);
}

#[test]
fn region_refuses_left_edge_wide_split_naming_grapheme() {
    // Row: 漢(lead+cont) then blanks. Crop starting at x=1 cuts continuation.
    let mut cells = blank_cells(4, 1);
    cells[0].symbol = "漢".into();
    cells[0].width = 2;
    cells[1].width = 0;
    cells[1].continuation = true;
    cells[1].symbol.clear();
    let parent = Screen::validate(4, 1, 0, 0, cells, Cursor::default())
        .expect("Screen::validate(4, 1, 0, 0, cells, Cursor::default()) succeeds");
    let err = parent
        .region(1, 0, 2, 1, RegionPolicy::Clip)
        .expect_err("parent.region(1, 0, 2, 1, RegionPolicy::Clip) is an error");
    assert!(err.0.contains('漢'), "{err}");
    assert!(err.0.contains("splits wide grapheme"), "{err}");
}

#[test]
fn region_refuses_right_edge_wide_split_naming_grapheme() {
    // Crop ending right after a wide lead strands its continuation.
    let mut cells = blank_cells(4, 1);
    cells[1].symbol = "漢".into();
    cells[1].width = 2;
    cells[2].width = 0;
    cells[2].continuation = true;
    cells[2].symbol.clear();
    let parent = Screen::validate(4, 1, 0, 0, cells, Cursor::default())
        .expect("Screen::validate(4, 1, 0, 0, cells, Cursor::default()) succeeds");
    let err = parent
        .region(0, 0, 2, 1, RegionPolicy::Clip)
        .expect_err("parent.region(0, 0, 2, 1, RegionPolicy::Clip) is an error");
    assert!(err.0.contains('漢'), "{err}");
    assert!(err.0.contains("splits wide grapheme"), "{err}");
}

#[test]
fn region_mask_policy_recorded() {
    let parent = Screen::blank(4, 4);
    let r = parent
        .region(0, 0, 2, 2, RegionPolicy::Mask)
        .expect("parent.region(0, 0, 2, 2, RegionPolicy::Mask) succeeds");
    assert_eq!(r.policy(), RegionPolicy::Mask);
    assert_eq!((r.screen().cols(), r.screen().rows()), (2, 2));
}

#[test]
fn region_rejects_out_of_bounds_and_zero() {
    let parent = Screen::blank(4, 4);
    assert!(parent.region(3, 3, 2, 2, RegionPolicy::Clip).is_err());
    assert!(parent.region(0, 0, 0, 2, RegionPolicy::Clip).is_err());
}
