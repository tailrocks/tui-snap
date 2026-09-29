//! Capture helpers shared by `format_contracts` + `interaction_contracts`.
//!
//! Included via `#[path]` only where every helper is used, so no suite
//! compiles an unused helper (see `common/mod.rs`).

use std::path::PathBuf;
use tuiscotti_render::profile::Profile;
use tuiscotti_render::render::Renderer;

/// Default render profile.
#[must_use]
pub(crate) fn profile() -> Profile {
    Profile::default_profile()
}

/// Fresh renderer over the vendored faces.
pub(crate) fn renderer() -> anyhow::Result<Renderer> {
    Ok(profile().renderer(&tuiscotti::VENDORED_FACES)?)
}

/// Read a committed `tests/fixtures/expected` baseline.
pub(crate) fn read_expected(name: &str) -> std::io::Result<String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/expected")
        .join(name);
    std::fs::read_to_string(&path)
}
