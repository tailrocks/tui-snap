//! Render-cache qualification: behavior, fingerprint, rejection.
//!
//! Split into one module per area so each file stays under the repo line
//! gate; behavior is unchanged.

#[path = "cache/behavior.rs"]
mod behavior;
#[path = "cache/fingerprint.rs"]
mod fingerprint;
#[path = "cache/helpers.rs"]
mod helpers;
#[path = "cache/rejection.rs"]
mod rejection;
