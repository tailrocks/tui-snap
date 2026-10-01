//! Live-PTY helpers used only by `interaction_contracts`.
//!
//! Included via `#[path]` only where every helper is used, so no suite
//! compiles an unused helper (see `common/mod.rs`).

use std::path::PathBuf;

/// Authoritative path of a `*_fixture` binary: the runtime environment
/// first (`tuiscotti::runner::resolve_bin`, correct under nextest
/// archive/remap runs), else the compile-time `CARGO_BIN_EXE_<name>` cargo
/// bakes into this test target. No probing, no nested cargo builds.
pub(crate) fn fixture_bin(name: &str) -> anyhow::Result<PathBuf> {
    if let Ok(path) = tuiscotti::runner::resolve_bin("tuiscotti-fixtures", name) {
        return Ok(path);
    }
    let compiled = match name {
        "menu_fixture" => env!("CARGO_BIN_EXE_menu_fixture"),
        "streams_fixture" => env!("CARGO_BIN_EXE_streams_fixture"),
        "protocol_fixture" => env!("CARGO_BIN_EXE_protocol_fixture"),
        _ => return Err(anyhow::anyhow!("unknown fixture binary {name:?}")),
    };
    Ok(PathBuf::from(compiled))
}

/// Fresh unique scratch directory under the platform temp dir.
pub(crate) fn scratch_dir(prefix: &str) -> anyhow::Result<PathBuf> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "tuiscotti-g5-{prefix}-{}-{nanos}",
        std::process::id(),
    ));
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}
