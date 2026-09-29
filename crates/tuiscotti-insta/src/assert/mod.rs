//! Public assertion facade: snapshot/screenshot macros + frozen policy (M2: I01, I02, I06, I07).
//!
//! - [`crate::assert_snapshot!`]: styled canonical state ([`crate::insta_proto::insta_string`])
//!   through native Insta review, with a content-derived generation binding carried in
//!   the snapshot description.
//! - [`crate::assert_screenshot!`]: canonical state PLUS an independently rendered PNG as ONE
//!   sample. The full candidate bundle (canonical + tagged image +
//!   ANSI/TXT/HTML + manifest) is published BEFORE any failure; the PNG is
//!   compared by decoded pixels ([`crate::insta_proto::PngPixelComparator`]);
//!   a binding mismatch between the accepted canonical and PNG snapshots
//!   fails via [`check_consistent`]. Both Insta assertions run even when the
//!   first fails, so one run yields BOTH pendings; failures aggregate.
//! - [`Policy`]: [`Policy::Evolving`] is the Insta review flow above;
//!   [`Policy::Frozen`] pins a read-only directory of approved canonical+PNG files that
//!   rejects acceptance and never self-heals.
//! - [`emit_four`] / [`import_frozen_v1`]: four-artifact (ANSI/TXT/PNG/HTML) export from
//!   one [`Screen`](tuiscotti_core::screen::Screen) and a read-only importer for classic/grouped four-file trees.
//!
//! Design notes (resolved snapshot identity):
//! - The macros textually expand `$crate::insta::assert_snapshot!` /
//!   `assert_binary_snapshot!` AT THE CALLER, so Insta captures the caller's
//!   `file!()`/`module_path!()`/`line!()` and its `CARGO_MANIFEST_DIR` for
//!   snapshot placement: the `.snap` `source:` field names the caller. Each
//!   argument is evaluated exactly once into a hygienic `__tuiscotti_*` binding.
//! - [`SnapshotIdentity`] resolves the SAME inputs Insta uses (caller
//!   manifest dir via caller-expanded `env!`, caller file parent, snapshot
//!   path, active suffix) into one absolute directory plus suffixed file
//!   stems, used end-to-end for the Insta settings, the consistency gate,
//!   and the evidence manifest. The gate can never read different files
//!   than Insta compared.
//! - Scoped settings derive from [`insta::Settings::clone_current`]: an outer
//!   `snapshot_suffix` (parameterized tests) is honored by Insta AND by the
//!   identity/gate (`{name}@{suffix}`), while the snapshot path,
//!   module-prepend, description, and PNG comparator are overridden inside a
//!   `bind` scope that never leaks outward.
//! - The caller location is ALSO embedded in the snapshot description
//!   (`... at <file>:<line>`), which review tools display, together with the
//!   render identity (`<profile>/rv<version>/<alpha>`) the PNG verdict
//!   depends on.
//! - New-format samples carry a MANDATORY complete compound binding
//!   ([`sample_binding`]: canonical + render identity + PNG payload); the
//!   macro path enforces it with the strict [`check_consistent`] gate —
//!   missing or partial bindings fail instead of passing half-blind.
//!   Read-only import of old-format trees stays explicit
//!   ([`import_frozen_v1`], `tuiscotti_runtime::import_compat`).
//! - Insta exposes no `Settings` switch for the update behavior; forbid
//!   auto-write with `INSTA_UPDATE=no` in the process environment before the
//!   first assertion (Insta memoizes tool config per workspace binary).
//!
//! Environment:
//! - [`SNAPSHOT_DIR_ENV`]: explicit Insta snapshot directory (absolute, or
//!   relative to the caller dir like Insta's own join).
//! - [`EVIDENCE_DIR_ENV`]: candidate-evidence root for [`crate::assert_screenshot!`] (default
//!   `target/tuiscotti-evidence`). Bundles land in
//!   `<root>/<package>/<test>/<scenario>[@<variant>]/<run>/<attempt>/`.
//! - [`SHARD_ENV`](evidence::SHARD_ENV): explicit shard label for evidence paths.
//!
//! Only public Insta APIs are used; there is no Insta fork or private clone.

/// Env var overriding the Insta snapshot directory for the facade macros.
pub const SNAPSHOT_DIR_ENV: &str = "TUISCOTTI_SNAPSHOT_DIR";
/// Env var overriding the candidate-evidence root for [`crate::assert_screenshot!`].
pub const EVIDENCE_DIR_ENV: &str = "TUISCOTTI_EVIDENCE_DIR";
/// PNG `tEXt` keyword carrying the sample binding inside the PNG bytes.
pub const PNG_GEN_KEYWORD: &str = "tuiscotti:generation";
/// Snapshot-description prefix carrying the sample binding (parse: first token).
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

pub use evidence::{
    AttemptIdentity, EvidenceId, SHARD_ENV, current_test_name, png_generation, png_tag_generation,
    sanitize_segment,
};
pub(crate) use evidence::{BundlePayload, snap_generation, write_bundle_in};
pub use frozen::{
    ConsistencyError, EmittedPaths, FrozenError, Policy, assert_frozen_screenshot,
    assert_frozen_snapshot, check_consistent, check_frozen_screenshot, check_frozen_snapshot,
    emit_four, frozen_accept,
};
pub(crate) use import::check_scenario_name;
pub use import::{FrozenTree, ImportError, ImportedScenario, import_frozen_v1};
pub use macros::Location;
pub(crate) use paths::{description_for, render_identity};
pub use paths::{
    SnapshotIdentity, active_snapshot_suffix, evidence_dir, generation_id, resolve_snapshot_identity,
    resolve_snapshot_identity_in, sample_binding, snapshot_dir_override, snapshot_file_stem,
    suffixed_name,
};
pub use sample::{
    AssertError, PreparedScreenshot, Sample, aggregate_compound_result, frame_from_screen,
    panic_message, png_comparator, png_snapshot_base, prepare_screenshot, prepare_snapshot,
    render_sample, screenshot_png_comparator, snapshot_settings,
};
pub use underline::{assert_underline_at, check_underline_at};
