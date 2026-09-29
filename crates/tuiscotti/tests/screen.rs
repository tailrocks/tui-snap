//! M1 Screen/Observation model: validation, regions, identity (M01/M02/M04/M07/M08).

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use tuiscotti::frame::MAX_DIM;
use tuiscotti::{
    CaptureProvenance, CaptureReason, Cell, Color, Cursor, CursorStyle, Frame, Maybe, Observation,
    Provenance, RegionPolicy, Rgb, Screen, TermState,
};

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

fn blank_cells(cols: u16, rows: u16) -> Vec<Cell> {
    let mut cells = Vec::new();
    for y in 0..rows {
        for x in 0..cols {
            cells.push(Cell::blank(x, y));
        }
    }
    cells
}

fn cap_prov() -> CaptureProvenance {
    CaptureProvenance::new(1_700_000_000_000, Some(1234), Some("/tmp/a".into()), 0)
}

fn hash_of<T: Hash>(v: &T) -> u64 {
    let mut h = DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}

#[test]
fn validate_ok_basic() {
    let s = Screen::validate(4, 2, 0, 0, blank_cells(4, 2), Cursor::default()).unwrap();
    assert_eq!((s.cols(), s.rows(), s.origin()), (4, 2, (0, 0)));
    assert_eq!(s.cells().len(), 8);
    assert_eq!(s.get(3, 1).unwrap().symbol, " ");
    assert!(s.get(4, 0).is_none());
}

#[test]
fn validate_1x1_ok() {
    // M04: no PTY minimums.
    let s = Screen::validate(1, 1, 0, 0, blank_cells(1, 1), Cursor::default()).unwrap();
    assert_eq!((s.cols(), s.rows()), (1, 1));
}

#[test]
fn validate_1col_and_1row_ok() {
    Screen::validate(1, 5, 0, 0, blank_cells(1, 5), Cursor::default()).unwrap();
    Screen::validate(5, 1, 0, 0, blank_cells(5, 1), Cursor::default()).unwrap();
}

#[test]
fn validate_nonzero_origin_ok() {
    let s = Screen::validate(3, 2, -40, 17, blank_cells(3, 2), Cursor::default()).unwrap();
    assert_eq!(s.origin(), (-40, 17));
}

#[test]
fn validate_rejects_zero_dims() {
    assert!(Screen::validate(0, 2, 0, 0, vec![], Cursor::default()).is_err());
    assert!(Screen::validate(2, 0, 0, 0, vec![], Cursor::default()).is_err());
}

#[test]
fn validate_rejects_oversize_dims() {
    let err = Screen::validate(MAX_DIM + 1, 1, 0, 0, vec![], Cursor::default()).unwrap_err();
    assert!(err.0.contains("exceed max"), "{err}");
}

#[test]
fn validate_rejects_cell_count_mismatch() {
    let mut cells = blank_cells(2, 2);
    cells.pop();
    let err = Screen::validate(2, 2, 0, 0, cells, Cursor::default()).unwrap_err();
    assert!(err.0.contains("cell count"), "{err}");
}

#[test]
fn validate_rejects_out_of_range_coords() {
    let mut cells = blank_cells(2, 2);
    cells[0].x = 9;
    assert!(Screen::validate(2, 2, 0, 0, cells, Cursor::default()).is_err());
}

#[test]
fn validate_rejects_width_above_2() {
    let mut cells = blank_cells(2, 1);
    cells[0].width = 3;
    let err = Screen::validate(2, 1, 0, 0, cells, Cursor::default()).unwrap_err();
    assert!(err.0.contains("width 3"), "{err}");
}

#[test]
fn validate_rejects_continuation_nonempty_symbol() {
    // Wide lead + continuation, but continuation carries a symbol.
    let mut cells = blank_cells(2, 1);
    cells[0].symbol = "漢".into();
    cells[0].width = 2;
    cells[1].width = 0;
    cells[1].continuation = true;
    cells[1].symbol = "x".into();
    let err = Screen::validate(2, 1, 0, 0, cells, Cursor::default()).unwrap_err();
    assert!(err.0.contains("empty symbol"), "{err}");
}

#[test]
fn validate_rejects_orphan_continuation_at_row_start() {
    let mut cells = blank_cells(2, 1);
    cells[0].width = 0;
    cells[0].continuation = true;
    cells[0].symbol.clear();
    let err = Screen::validate(2, 1, 0, 0, cells, Cursor::default()).unwrap_err();
    assert!(err.0.contains("orphan continuation"), "{err}");
}

#[test]
fn validate_rejects_orphan_continuation_wrong_lead_width() {
    let mut cells = blank_cells(2, 1);
    // cells[0] stays width 1: continuation has no wide lead.
    cells[1].width = 0;
    cells[1].continuation = true;
    cells[1].symbol.clear();
    let err = Screen::validate(2, 1, 0, 0, cells, Cursor::default()).unwrap_err();
    assert!(err.0.contains("orphan continuation"), "{err}");
}

#[test]
fn validate_accepts_wide_pair() {
    let mut cells = blank_cells(3, 1);
    cells[0].symbol = "漢".into();
    cells[0].width = 2;
    cells[1].width = 0;
    cells[1].continuation = true;
    cells[1].symbol.clear();
    Screen::validate(3, 1, 0, 0, cells, Cursor::default()).unwrap();
}

#[test]
fn validate_rejects_visible_cursor_outside_grid() {
    let cursor = Cursor {
        x: 5,
        y: 0,
        visible: true,
        style: CursorStyle::Block,
        blinking: false,
    };
    assert!(Screen::validate(2, 2, 0, 0, blank_cells(2, 2), cursor).is_err());
}

#[test]
fn maybe_never_conflates_unknown_unsupported_empty() {
    let unknown: Maybe<Vec<u16>> = Maybe::Unknown;
    let unsupported: Maybe<Vec<u16>> = Maybe::Unsupported;
    let empty: Maybe<Vec<u16>> = Maybe::Known(vec![]);
    assert_ne!(unknown, empty);
    assert_ne!(unsupported, empty);
    assert_ne!(unknown, unsupported);
    assert!(unknown.known().is_none());
    assert!(unsupported.known().is_none());
    assert_eq!(empty.known(), Some(&vec![]));
    let def_state = TermState::default();
    assert_eq!(def_state.modes, Maybe::Unknown);
    assert_eq!(def_state.title, Maybe::Unknown);
}

#[test]
fn observation_identity_determinism_provenance_excluded() {
    // M08: identical captures with different timestamps/PIDs/paths/attempts
    // compare equal and hash equal.
    let screen = Screen::blank(2, 2);
    let state = TermState {
        modes: Maybe::Known(vec![25]),
        palette: Maybe::Known(vec![(1, Rgb::new(1, 2, 3))]),
        title: Maybe::Known("app".into()),
        bells: Maybe::Known(0),
    };
    let a = Observation::new(
        screen.clone(),
        7,
        CaptureReason::Poll,
        state.clone(),
        CaptureProvenance::new(100, Some(11), Some("/a".into()), 0),
    );
    let b = Observation::new(
        screen,
        7,
        CaptureReason::Poll,
        state,
        CaptureProvenance::new(999, Some(22), Some("/b".into()), 3),
    );
    assert_eq!(a, b);
    assert_eq!(hash_of(&a), hash_of(&b));
}

#[test]
fn observation_content_differences_matter() {
    let base = || {
        Observation::new(
            Screen::blank(2, 2),
            7,
            CaptureReason::Poll,
            TermState::default(),
            cap_prov(),
        )
    };
    let a = base();
    // Different screen.
    let mut other = base();
    other.screen = Screen::blank(3, 3);
    assert_ne!(a, other);
    // Different revision.
    let mut other = base();
    other.revision = 8;
    assert_ne!(a, other);
    // Different reason.
    let mut other = base();
    other.reason = CaptureReason::Input;
    assert_ne!(a, other);
    // Different terminal state.
    let mut other = base();
    other.state.title = Maybe::Known("t".into());
    assert_ne!(a, other);
    // Unknown vs Known-empty state differs.
    let mut other = base();
    other.state.bells = Maybe::Known(0);
    let mut other2 = base();
    other2.state.bells = Maybe::Unknown;
    assert_ne!(other, other2);
}

#[test]
fn region_crop_preserves_geometry_and_origin() {
    let parent = Screen::validate(6, 4, 10, 20, blank_cells(6, 4), Cursor::default()).unwrap();
    let r = parent.region(2, 1, 3, 2, RegionPolicy::Clip).unwrap();
    assert_eq!(r.policy(), RegionPolicy::Clip);
    let s = r.screen();
    assert_eq!((s.cols(), s.rows()), (3, 2));
    assert_eq!(s.origin(), (12, 21));
    assert_eq!(s.get(0, 0).unwrap().x, 0);
    assert_eq!(s.get(2, 1).unwrap().y, 1);
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
    let parent = Screen::validate(4, 2, 0, 0, blank_cells(4, 2), cursor).unwrap();
    let inside = parent.region(2, 0, 2, 1, RegionPolicy::Clip).unwrap();
    let c = inside.screen().cursor();
    assert!(c.visible);
    assert_eq!((c.x, c.y), (1, 0));
    let outside = parent.region(0, 1, 2, 1, RegionPolicy::Clip).unwrap();
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
    let parent = Screen::validate(4, 1, 0, 0, cells, Cursor::default()).unwrap();
    let err = parent.region(1, 0, 2, 1, RegionPolicy::Clip).unwrap_err();
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
    let parent = Screen::validate(4, 1, 0, 0, cells, Cursor::default()).unwrap();
    let err = parent.region(0, 0, 2, 1, RegionPolicy::Clip).unwrap_err();
    assert!(err.0.contains('漢'), "{err}");
    assert!(err.0.contains("splits wide grapheme"), "{err}");
}

#[test]
fn region_mask_policy_recorded() {
    let parent = Screen::blank(4, 4);
    let r = parent.region(0, 0, 2, 2, RegionPolicy::Mask).unwrap();
    assert_eq!(r.policy(), RegionPolicy::Mask);
    assert_eq!((r.screen().cols(), r.screen().rows()), (2, 2));
}

#[test]
fn region_rejects_out_of_bounds_and_zero() {
    let parent = Screen::blank(4, 4);
    assert!(parent.region(3, 3, 2, 2, RegionPolicy::Clip).is_err());
    assert!(parent.region(0, 0, 0, 2, RegionPolicy::Clip).is_err());
}

#[test]
fn from_frame_roundtrip_preserves_source_distinctions() {
    // M02: styled blank + wide continuation + hidden/blink + cursor survive.
    let mut f = Frame::blank(4, 2, prov());
    let mut styled = Cell::blank(0, 0);
    styled.symbol = " ".into();
    styled.fg = Color::Rgb(Rgb::new(1, 2, 3));
    styled.bg = Color::Indexed(5);
    styled.mods.bold = true;
    styled.mods.hidden = true;
    styled.mods.blink = true;
    f.set(styled);
    let mut lead = Cell::blank(1, 0);
    lead.symbol = "漢".into();
    lead.width = 2;
    lead.mods.italic = true;
    f.set(lead);
    let mut cont = Cell::blank(2, 0);
    cont.symbol.clear();
    cont.width = 0;
    cont.continuation = true;
    f.set(cont);
    f.cursor = Cursor {
        x: 3,
        y: 1,
        visible: true,
        style: CursorStyle::Underline,
        blinking: true,
    };
    f.validate().unwrap();

    let s = Screen::from_frame(&f).unwrap();
    assert_eq!((s.cols(), s.rows(), s.origin()), (4, 2, (0, 0)));
    assert_eq!(s.cells(), &f.cells);
    assert_eq!(s.cursor(), &f.cursor);
    // Spot-check the preserved distinctions.
    let c = s.get(0, 0).unwrap();
    assert_eq!(c.fg, Color::Rgb(Rgb::new(1, 2, 3)));
    assert!(c.mods.hidden && c.mods.blink && c.mods.bold);
    assert_eq!(s.get(1, 0).unwrap().width, 2);
    assert!(s.get(2, 0).unwrap().continuation);
}

#[test]
fn from_frame_rejects_invalid_frame() {
    let mut f = Frame::blank(2, 2, prov());
    f.cells.pop();
    assert!(Screen::from_frame(&f).is_err());
}

#[test]
fn screen_is_immutable_snapshot_of_inputs() {
    // Mutating the input vec after validate must not affect the screen
    // (validate takes ownership; getters expose only shared refs).
    let cells = blank_cells(2, 2);
    let s = Screen::validate(2, 2, 0, 0, cells, Cursor::default()).unwrap();
    assert_eq!(s.cells().len(), 4);
}
