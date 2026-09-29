//! Pure-view helpers shared by `format_contracts` + `view_contracts`.
//!
//! Included via `#[path]` only where every helper is used, so no suite
//! compiles an unused helper (see `common/mod.rs`).

use tuiscotti_fixtures::views::Theme;

/// Pure-view protocol frame at `cols`×`rows`.
#[must_use]
pub(crate) fn protocol_frame(cols: u16, rows: u16, theme: Theme, empty: bool) -> tuiscotti::Frame {
    use tuiscotti_fixtures::views::protocol::Model;
    let model = if empty {
        Model::empty(theme, (cols, rows))
    } else {
        Model::demo(theme, (cols, rows))
    };
    tuiscotti::ratatui::draw_frame(cols, rows, crate::common::prov("protocol-view"), |f| {
        tuiscotti_fixtures::views::protocol::render(f, &model);
    })
}
