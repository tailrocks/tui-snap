//! Insta integration prototype (spike for backlog I01–I05; de-risks M2).
//!
//! This module is an experiment, not the final M2 API. It answers: can public
//! Insta APIs carry a compound canonical-plus-PNG snapshot lifecycle?
//!
//! - Canonical projections ([`tuiscotti_core::screen::canonical_string`] /
//!   [`tuiscotti_core::screen::canonical_value`]): deterministic [`Screen`](tuiscotti_core::screen::Screen)
//!   projections for `assert_snapshot!` / `assert_json_snapshot!` (I01). Pure
//!   functions over the screen model, so they live in core.
//! - [`PngPixelComparator`]: custom [`insta::Comparator`] doing decoded-pixel
//!   equality via [`tuiscotti_render::diff`] for for `assert_binary_snapshot!` (I02, I03).
//!
//! Qualified against insta **1.48.0** (see `Cargo.lock`), trait signature
//! verified against the compiled registry source
//! (`insta-1.48.0/src/comparator.rs`):
//!
//! ```text
//! pub trait Comparator: Send + Sync + 'static {
//!     fn matches(&self, reference: &Snapshot, test: &Snapshot) -> bool;
//!     fn matches_fully(&self, reference: &Snapshot, test: &Snapshot) -> bool { ... }
//!     fn dyn_clone(&self) -> Box<dyn Comparator>;
//! }
//! ```
//!
//! Public-API notes (see also the header of `tests/insta_spike.rs`):
//! - [`insta::Comparator`], [`insta::DefaultComparator`], [`insta::Settings`]
//!   and [`insta::Snapshot`] are root-public. [`insta::Snapshot::contents`]
//!   exposes the payload, but the [`insta::internals::SnapshotContents`] enum
//!   lives under `insta::internals` and there is no public
//!   `Snapshot::as_binary()` accessor — that is the one gap found (I05).
//! - `MetaData::snapshot_kind` (binary extension) is `pub(crate)`, so an
//!   external comparator cannot re-check extension equality the way
//!   `DefaultComparator` does. For decoded-pixel equality this is the correct
//!   behavior anyway: bytes that do not decode as PNG never match.

use tuiscotti_render::diff::AlphaPolicy;

/// Custom Insta comparator (I03): binary snapshots compare by **decoded**
/// pixels under an explicit [`AlphaPolicy`]; text snapshots delegate to
/// [`insta::DefaultComparator`] so canonical assertions keep exact stock
/// semantics (including legacy-format acceptance).
///
/// Never matches: undecodable/corrupt PNG on either side, a missing binary
/// sidecar (`Binary(None)`), text/binary kind mixes, or any decode error.
/// There is no threshold and no perceptual fallback — the verdict is
/// [`tuiscotti_render::diff::PixelVerdict::pixels_equal`].
#[derive(Debug, Clone, Copy, Default)]
pub struct PngPixelComparator {
    alpha_policy: AlphaPolicy,
}

impl PngPixelComparator {
    /// Comparator with an explicit alpha policy (`StraightRgba` default).
    #[must_use]
    pub fn new(alpha_policy: AlphaPolicy) -> Self {
        Self { alpha_policy }
    }

    /// The alpha policy this comparator decodes pixels under.
    #[must_use]
    pub fn alpha_policy(self) -> AlphaPolicy {
        self.alpha_policy
    }
}

impl insta::Comparator for PngPixelComparator {
    fn matches(&self, reference: &insta::Snapshot, test: &insta::Snapshot) -> bool {
        use insta::internals::SnapshotContents;
        match (reference.contents(), test.contents()) {
            (SnapshotContents::Binary(Some(a)), SnapshotContents::Binary(Some(b))) => {
                tuiscotti_render::diff::compare_png_with_alpha(a, b, self.alpha_policy)
                    .is_ok_and(|v| v.pixels_equal)
            }
            // Stock text semantics, untouched: no canonical field is ignored
            // or reinterpreted here.
            (SnapshotContents::Text(_), SnapshotContents::Text(_)) => {
                insta::DefaultComparator.matches(reference, test)
            }
            // Absent sidecar or kind mix: never a match.
            _ => false,
        }
    }

    fn dyn_clone(&self) -> Box<dyn insta::Comparator> {
        Box::new(*self)
    }
}
