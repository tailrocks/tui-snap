//! Shared helpers for the G5 contract suites (pure public-API consumers).
//!
//! Included via `#[path]` from `format_contracts`, `view_contracts`, and
//! `interaction_contracts`. Everything here goes through the public API:
//! `tuiscotti` (facade), `tuiscotti_core`, `tuiscotti_render`, and the
//! `tuiscotti_fixtures` views — the same crates an external consumer uses.

use std::path::PathBuf;
use tuiscotti_fixtures::views::Theme;
use tuiscotti_render::profile::Profile;
use tuiscotti_render::render::Renderer;

/// Deterministic provenance for pure-view captures.
#[must_use]
pub fn prov(source: &str) -> tuiscotti::Provenance {
    tuiscotti::Provenance {
        tool: "tuiscotti-fixtures".to_string(),
        tool_version: env!("CARGO_PKG_VERSION").to_string(),
        profile: "tuiscotti-default".to_string(),
        source: source.to_string(),
        argv: Vec::new(),
        created_unix: 0,
    }
}

/// Default render profile.
#[must_use]
pub fn profile() -> Profile {
    Profile::default_profile()
}

/// Fresh renderer over the vendored faces.
#[must_use]
pub fn renderer() -> Renderer {
    profile()
        .renderer(&tuiscotti::VENDORED_FACES)
        .expect("vendored renderer builds")
}

/// Pure-view menu frame at `cols`×`rows`.
#[must_use]
pub fn menu_frame(
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
pub fn streams_frame(cols: u16, rows: u16, theme: Theme, empty: bool) -> tuiscotti::Frame {
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

/// Pure-view protocol frame at `cols`×`rows`.
#[must_use]
pub fn protocol_frame(cols: u16, rows: u16, theme: Theme, empty: bool) -> tuiscotti::Frame {
    use tuiscotti_fixtures::views::protocol::Model;
    let model = if empty {
        Model::empty(theme, (cols, rows))
    } else {
        Model::demo(theme, (cols, rows))
    };
    tuiscotti::ratatui::draw_frame(cols, rows, prov("protocol-view"), |f| {
        tuiscotti_fixtures::views::protocol::render(f, &model);
    })
}

/// Read a committed `tests/fixtures/data` file.
#[must_use]
pub fn read_data(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/data")
        .join(name);
    std::fs::read(&path).expect("fixture data ships with the crate")
}

/// Read a committed `tests/fixtures/expected` baseline.
#[must_use]
pub fn read_expected(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/expected")
        .join(name);
    std::fs::read_to_string(&path).expect("expected baseline ships with the crate")
}

/// Authoritative path of a `*_fixture` binary: the runtime environment
/// first ([`tuiscotti::runner::resolve_bin`], correct under nextest
/// archive/remap runs), else the compile-time `CARGO_BIN_EXE_<name>` cargo
/// bakes into this test target. No probing, no nested cargo builds.
#[must_use]
pub fn fixture_bin(name: &str) -> PathBuf {
    if let Ok(path) = tuiscotti::runner::resolve_bin("tuiscotti-fixtures", name) {
        return path;
    }
    let compiled = match name {
        "menu_fixture" => env!("CARGO_BIN_EXE_menu_fixture"),
        "streams_fixture" => env!("CARGO_BIN_EXE_streams_fixture"),
        "protocol_fixture" => env!("CARGO_BIN_EXE_protocol_fixture"),
        _ => panic!("unknown fixture binary {name:?}"),
    };
    PathBuf::from(compiled)
}

/// Fresh unique scratch directory under the platform temp dir.
#[must_use]
pub fn scratch_dir(prefix: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "tuiscotti-g5-{prefix}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("wall clock")
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}
