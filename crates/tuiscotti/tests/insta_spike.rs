//! Compound-snapshot integration SPIKE (backlog I01–I05; de-risks M2).
//!
//! Question: can public Insta APIs carry a compound canonical-plus-PNG
//! snapshot lifecycle where both artifacts bind to one candidate generation?
//!
//! What public APIs sufficed:
//! - `insta::assert_snapshot!` (canonical text) and
//!   `insta::assert_binary_snapshot!("name.png", bytes)` (PNG sidecar).
//! - `insta::Settings::{set_snapshot_path, set_description,
//!   set_prepend_module_to_snapshot, set_comparator}` + `bind` for hermetic
//!   per-phase scopes (absolute tempdirs; `Path::join` with an absolute path
//!   replaces the default `tests/snapshots` location).
//! - `insta::Comparator` + `insta::DefaultComparator` (text delegation) for
//!   the decoded-pixel comparator. `INSTA_UPDATE` stays ambient (read-only):
//!   `set_var` is an `unsafe fn` in edition 2024 and cannot be used under the
//!   workspace lints. Pending-dependent simulations (accept/reject/interrupted)
//!   need the `new` behavior — failing assertions write `.snap.new` pendings
//!   AND fail without blessing — so they skip unless the effective mode
//!   writes pendings (see `common`); run with `INSTA_UPDATE=new` to force it.
//! - `insta::Snapshot::from_file` for direct comparator unit checks.
//!
//! Missing hook (I05): there is NO public `Snapshot::as_binary()` accessor.
//! Payload extraction needs `insta::internals::SnapshotContents` (public but
//! explicitly internal), and `MetaData::snapshot_kind` (binary extension) is
//! `pub(crate)`, so an external comparator cannot re-check extension equality
//! like `DefaultComparator` does. Upstream ask: `as_binary() -> Option<&[u8]>`.
//! `assert_json_snapshot!` over `insta_value` needs only the `json` feature,
//! but Insta serializes via its own `Content` pretty-printer (not reproducible
//! outside Insta), so the structured projection is pinned by direct asserts,
//! not by an approved JSON file.
//!
//! Review/reject flows tested (via file-level simulation of
//! `cargo insta accept` = rename `.snap.new` → `.snap` incl. sidecar, and
//! `cargo insta reject` = delete `.snap.new` files):
//! - green: canonical + PNG approved at one generation pass together (I01, I02).
//! - reject: accept canonical, reject PNG → mixed baseline fails re-run (I04).
//! - partial accept: accept canonical, leave PNG pending → re-run fails (I04).
//! - interrupted write: torn binary pending (sidecar lost) → accept refused,
//!   post-crash mixed baseline fails re-run, pending regenerates (C08, I04).
//!
//! Each generation is bound by a `generation_id` embedded in BOTH artifacts:
//! the `description` field of each `.snap` file (set via public
//! `Settings::set_description`, visible in review) and a `tEXt` chunk inside
//! the PNG bytes. [`check_consistent`] fails on any mismatch.
//!
//! Simulation artifact: Insta auto-suffixes repeat assertions of one name in
//! a single process (`name-2`), with no public opt-out — the dedup key is
//! `module::name` and does NOT include the test function, so names must also
//! be unique across `#[test]`s in one binary. Re-run phases therefore use
//! FRESH snapshot names over byte-copied approved state. Real M2 re-runs are
//! separate processes and unaffected.
//!
//! Adjacent finding (out of spike scope, needs a P0 owner): `crate::diff`'s
//! identical-bytes fast path returns `pixels_equal: true` without consulting
//! the [`AlphaPolicy`], so byte-identical semi-transparent PNGs pass under
//! `Opaque` although the policy demands failure on any non-255 alpha. The
//! policy test below uses distinctly-encoded identical pixels to exercise the
//! real decoded-pixel path.
//!
//! These tests never touch `tests/visual/approved`: all snapshot dirs are
//! per-test tempdirs.

mod common;

#[path = "insta_spike/fixtures.rs"]
mod fixtures;
#[path = "insta_spike/harness.rs"]
mod harness;
#[path = "insta_spike/lifecycle.rs"]
mod lifecycle;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

const GEN1: &str = "gen-001";
const GEN2: &str = "gen-002";
const PNG_GEN_KEYWORD: &str = "tuisnap:generation";
