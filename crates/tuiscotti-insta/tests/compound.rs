//! G6 compound approval tests: same-sample canonical+PNG binding.
//!
//! - Canonical-identical/render-different repro: one [`Screen`] rendered
//!   under two profiles shares its canonical text but yields different PNG
//!   bytes, and the decoded-pixel comparison fails (pixel equality is never
//!   inferred from cells).
//! - Partial acceptance: a canonical approval without its PNG partner (or
//!   with a mismatched generation) fails the strict [`check_consistent`]
//!   gate instead of passing half-blind.
//! - Evidence precedes failure: [`assert_screenshot!`] writes all four
//!   artifacts before the Insta assertions can fail.
//! - Render identity: snapshot descriptions record the profile, renderer
//!   version, and alpha policy the PNG verdict depends on.
//!
//! Split into one module per area so each file stays under the repo line
//! gate; behavior is unchanged.

#[path = "compound/binding.rs"]
mod binding;
#[path = "compound/evidence.rs"]
mod evidence;
#[path = "compound/helpers.rs"]
mod helpers;
#[path = "compound/identity.rs"]
mod identity;
#[path = "compound/render_identity.rs"]
mod render_identity;
