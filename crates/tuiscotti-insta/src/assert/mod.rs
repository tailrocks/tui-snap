//! Public assertion facade: snapshot/screenshot macros + frozen policy (M2: I01, I02, I06, I07).
//!
//! - [`crate::assert_snapshot!`]: styled canonical state ([`crate::insta_proto::insta_string`])
//!   through native Insta review, with a content-derived generation binding carried in
//!   the snapshot description.
//! - [`crate::assert_screenshot!`]: canonical state PLUS an independently rendered PNG as ONE
//!   sample. Candidate evidence (PNG + ANSI/TXT/HTML from the same sample) is written
//!   BEFORE any failure; the PNG is compared by decoded pixels
//!   ([`crate::insta_proto::PngPixelComparator`]); a generation mismatch between the
//!   accepted canonical and PNG snapshots fails via [`check_consistent`].
//! - [`Policy`]: [`Policy::Evolving`] is the Insta review flow above;
//!   [`Policy::Frozen`] pins a read-only directory of approved canonical+PNG files that
//!   rejects acceptance and never self-heals.
//! - [`emit_four`] / [`import_frozen_v1`]: four-artifact (ANSI/TXT/PNG/HTML) export from
//!   one [`Screen`](tuiscotti_core::screen::Screen) and a read-only importer for classic/grouped four-file trees.
//!
//! Design notes (G6 caller-fixed metadata):
//! - The macros textually expand `$crate::insta::assert_snapshot!` /
//!   `assert_binary_snapshot!` AT THE CALLER, so Insta captures the caller's
//!   `file!()`/`module_path!()`/`line!()` and its `CARGO_MANIFEST_DIR` for
//!   snapshot placement: the `.snap` `source:` field names the caller, and
//!   relative snapshot dirs resolve against the caller's crate. Each argument
//!   is evaluated exactly once into a hygienic `__tuiscotti_*` binding.
//! - Scoped settings derive from [`insta::Settings::clone_current`]: an outer
//!   `snapshot_suffix` (parameterized tests) is honored, while the snapshot
//!   path, module-prepend, description, and PNG comparator are overridden
//!   inside a `bind` scope that never leaks outward.
//! - The caller location is ALSO embedded in the snapshot description
//!   (`... at <file>:<line>`), which review tools display, together with the
//!   render identity (`<profile>/rv<version>/<alpha>`) the PNG verdict
//!   depends on. The default snapshot directory is derived from the CALLER
//!   file (`<caller-dir>/snapshots`, mirroring Insta's native default); set
//!   [`SNAPSHOT_DIR_ENV`] to override.
//! - `insta_proto` carries no `check_consistent` (it only ever existed as a local helper
//!   in `tests/insta_spike.rs`), and this facade may not touch that module — so the
//!   canonical consistency gate lives here ([`check_consistent`]).
//! - Insta exposes no `Settings` switch for the update behavior; forbid
//!   auto-write with `INSTA_UPDATE=no` in the process environment before the
//!   first assertion (Insta memoizes tool config per workspace binary).
//!
//! Environment:
//! - [`SNAPSHOT_DIR_ENV`]: explicit Insta snapshot directory (tests point it at a
//!   tempdir; unset means the caller-derived default above).
//! - [`EVIDENCE_DIR_ENV`]: candidate-evidence root for [`crate::assert_screenshot!`] (default
//!   `target/tuiscotti-evidence`). Files are `<name>.{png,ansi,txt,html}`.

/// Env var overriding the Insta snapshot directory for the facade macros.
pub const SNAPSHOT_DIR_ENV: &str = "TUISCOTTI_SNAPSHOT_DIR";
/// Env var overriding the candidate-evidence root for [`crate::assert_screenshot!`].
pub const EVIDENCE_DIR_ENV: &str = "TUISCOTTI_EVIDENCE_DIR";
/// PNG `tEXt` keyword carrying the sample generation inside the PNG bytes.
pub const PNG_GEN_KEYWORD: &str = "tuiscotti:generation";
/// Snapshot-description prefix carrying the sample generation (parse: first token).
pub const GEN_DESC_PREFIX: &str = "tuiscotti generation ";
/// Suffix mapping a screenshot name to its PNG snapshot base (`<name>-img`).
pub const PNG_SNAPSHOT_SUFFIX: &str = "-img";
/// File stem used by [`emit_four`] (`snapshot.{ansi,txt,png,html}`).
pub const FOUR_STEM: &str = "snapshot";

pub mod evidence;
pub mod frozen;
pub mod import;
pub mod macros;
pub mod paths;
pub mod sample;
pub mod underline;

pub use evidence::{png_generation, png_tag_generation};
pub(crate) use evidence::{snap_generation, write_evidence_in};
pub use frozen::{
    ConsistencyError, EmittedPaths, FrozenError, Policy, assert_frozen_screenshot,
    assert_frozen_snapshot, check_consistent, check_consistent_lenient, check_frozen_screenshot,
    check_frozen_snapshot, emit_four, frozen_accept,
};
pub(crate) use import::check_scenario_name;
pub use import::{FrozenTree, ImportError, ImportedScenario, import_frozen_v1};
pub use macros::Location;
pub(crate) use paths::description_for;
pub use paths::{default_snapshot_dir, evidence_dir, generation_id};
pub use sample::{
    AssertError, PreparedScreenshot, Sample, frame_from_screen, png_comparator, png_snapshot_base,
    prepare_screenshot, prepare_snapshot, render_sample, screenshot_png_comparator,
    snapshot_settings,
};
pub use underline::{assert_underline_at, check_underline_at};
