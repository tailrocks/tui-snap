//! Helpers shared by every G5 contract suite (pure public-API consumers).
//!
//! Included via `#[path]` from `format_contracts`, `view_contracts`, and
//! `interaction_contracts`. Everything here goes through the public API:
//! `tuiscotti` (facade), `tuiscotti_core`, `tuiscotti_render`, and the
//! `tuiscotti_fixtures` views — the same crates an external consumer uses.
//!
//! Only helpers used (directly or transitively) by *all three* suites live
//! here. Suite-pair helpers live in sibling modules included only where
//! used — `pure` (`format_contracts` + `view_contracts`), `capture`
//! (`format_contracts` + `interaction_contracts`) — and live-PTY helpers in
//! `live` (`interaction_contracts` only), so no suite compiles an unused
//! helper.

use std::path::PathBuf;
use tuiscotti_fixtures::views::Theme;

/// Deterministic provenance for pure-view captures.
#[must_use]
pub(crate) fn prov(source: &str) -> tuiscotti::Provenance {
    tuiscotti::Provenance {
        tool: "tuiscotti-fixtures".to_string(),
        tool_version: env!("CARGO_PKG_VERSION").to_string(),
        profile: "tuiscotti-default".to_string(),
        source: source.to_string(),
        argv: Vec::new(),
        created_unix: 0,
    }
}

/// Pure-view menu frame at `cols`×`rows`.
#[must_use]
pub(crate) fn menu_frame(
    cols: u16,
    rows: u16,
    theme: Theme,
    scenario: tuiscotti_fixtures::driver::Scenario,
) -> tuiscotti::Frame {
    use tuiscotti_fixtures::driver::Scenario;
    use tuiscotti_fixtures::views::menu::Model;
    let model = match scenario {
        Scenario::Demo => Model::demo(theme),
        Scenario::Empty => Model::empty(theme),
        Scenario::Error => Model::with_error(theme, "boom: deterministic error"),
    };
    tuiscotti::ratatui::draw_frame(cols, rows, prov("menu-view"), |f| {
        tuiscotti_fixtures::views::menu::render(f, &model);
    })
}

/// Pure-view streams frame at `cols`×`rows`.
#[must_use]
pub(crate) fn streams_frame(cols: u16, rows: u16, theme: Theme, empty: bool) -> tuiscotti::Frame {
    use tuiscotti_fixtures::views::streams::Model;
    let model = if empty {
        Model::empty(theme)
    } else {
        Model::demo(theme)
    };
    tuiscotti::ratatui::draw_frame(cols, rows, prov("streams-view"), |f| {
        tuiscotti_fixtures::views::streams::render(f, &model);
    })
}

/// Read a committed `tests/fixtures/data` file.
pub(crate) fn read_data(name: &str) -> std::io::Result<Vec<u8>> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/data")
        .join(name);
    std::fs::read(&path)
}
