//! M1 Screen/Observation model: validation, regions, identity (M01/M02/M04/M07/M08).

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use tuiscotti::frame::MAX_DIM;
use tuiscotti::{
    CaptureProvenance, CaptureReason, Cell, Color, Cursor, CursorStyle, Frame, Maybe, Observation,
    Provenance, RegionPolicy, Rgb, Screen, TermState,
};

#[path = "screen/region.rs"]
mod region;

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
    let s = Screen::validate(4, 2, 0, 0, blank_cells(4, 2), Cursor::default())
        .expect("Screen::validate(4, 2, 0, 0, blank_cells(4, 2), Cursor::default()) succeeds");
    assert_eq!((s.cols(), s.rows(), s.origin()), (4, 2, (0, 0)));
    assert_eq!(s.cells().len(), 8);
    assert_eq!(s.get(3, 1).expect("s.get(3, 1) is some").symbol, " ");
    assert!(s.get(4, 0).is_none());
}

#[test]
fn validate_1x1_ok() {
    // M04: no PTY minimums.
    let s = Screen::validate(1, 1, 0, 0, blank_cells(1, 1), Cursor::default())
        .expect("Screen::validate(1, 1, 0, 0, blank_cells(1, 1), Cursor::default()) succeeds");
    assert_eq!((s.cols(), s.rows()), (1, 1));
}

#[test]
fn validate_1col_and_1row_ok() {
    Screen::validate(1, 5, 0, 0, blank_cells(1, 5), Cursor::default())
        .expect("Screen::validate(1, 5, 0, 0, blank_cells(1, 5), Cursor::default()) succeeds");
    Screen::validate(5, 1, 0, 0, blank_cells(5, 1), Cursor::default())
        .expect("Screen::validate(5, 1, 0, 0, blank_cells(5, 1), Cursor::default()) succeeds");
}

#[test]
fn validate_nonzero_origin_ok() {
    let s = Screen::validate(3, 2, -40, 17, blank_cells(3, 2), Cursor::default())
        .expect("Screen::validate(3, 2, -40, 17, blank_cells(3, 2), Cursor::default()) succeeds");
    assert_eq!(s.origin(), (-40, 17));
}

#[test]
fn validate_rejects_zero_dims() {
    assert!(Screen::validate(0, 2, 0, 0, vec![], Cursor::default()).is_err());
    assert!(Screen::validate(2, 0, 0, 0, vec![], Cursor::default()).is_err());
}

#[test]
fn validate_rejects_oversize_dims() {
    let err = Screen::validate(MAX_DIM + 1, 1, 0, 0, vec![], Cursor::default()).expect_err(
        "Screen::validate(MAX_DIM + 1, 1, 0, 0, vec![], Cursor::default()) is an error",
    );
    assert!(err.0.contains("exceed max"), "{err}");
}

#[test]
fn validate_rejects_cell_count_mismatch() {
    let mut cells = blank_cells(2, 2);
    cells.pop();
    let err = Screen::validate(2, 2, 0, 0, cells, Cursor::default())
        .expect_err("Screen::validate(2, 2, 0, 0, cells, Cursor::default()) is an error");
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
    let err = Screen::validate(2, 1, 0, 0, cells, Cursor::default())
        .expect_err("Screen::validate(2, 1, 0, 0, cells, Cursor::default()) is an error");
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
    let err = Screen::validate(2, 1, 0, 0, cells, Cursor::default())
        .expect_err("Screen::validate(2, 1, 0, 0, cells, Cursor::default()) is an error");
    assert!(err.0.contains("empty symbol"), "{err}");
}

#[test]
fn validate_rejects_orphan_continuation_at_row_start() {
    let mut cells = blank_cells(2, 1);
    cells[0].width = 0;
    cells[0].continuation = true;
    cells[0].symbol.clear();
    let err = Screen::validate(2, 1, 0, 0, cells, Cursor::default())
        .expect_err("Screen::validate(2, 1, 0, 0, cells, Cursor::default()) is an error");
    assert!(err.0.contains("orphan continuation"), "{err}");
}

#[test]
fn validate_rejects_orphan_continuation_wrong_lead_width() {
    let mut cells = blank_cells(2, 1);
    // cells[0] stays width 1: continuation has no wide lead.
    cells[1].width = 0;
    cells[1].continuation = true;
    cells[1].symbol.clear();
    let err = Screen::validate(2, 1, 0, 0, cells, Cursor::default())
        .expect_err("Screen::validate(2, 1, 0, 0, cells, Cursor::default()) is an error");
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
    Screen::validate(3, 1, 0, 0, cells, Cursor::default())
        .expect("Screen::validate(3, 1, 0, 0, cells, Cursor::default()) succeeds");
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
    f.validate().expect("f.validate() succeeds");

    let s = Screen::from_frame(&f).expect("Screen::from_frame(&f) succeeds");
    assert_eq!((s.cols(), s.rows(), s.origin()), (4, 2, (0, 0)));
    assert_eq!(s.cells(), &f.cells);
    assert_eq!(s.cursor(), &f.cursor);
    // Spot-check the preserved distinctions.
    let c = s.get(0, 0).expect("s.get(0, 0) is some");
    assert_eq!(c.fg, Color::Rgb(Rgb::new(1, 2, 3)));
    assert!(c.mods.hidden && c.mods.blink && c.mods.bold);
    assert_eq!(s.get(1, 0).expect("s.get(1, 0) is some").width, 2);
    assert!(s.get(2, 0).expect("s.get(2, 0) is some").continuation);
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
    let s = Screen::validate(2, 2, 0, 0, cells, Cursor::default())
        .expect("Screen::validate(2, 2, 0, 0, cells, Cursor::default()) succeeds");
    assert_eq!(s.cells().len(), 4);
}
