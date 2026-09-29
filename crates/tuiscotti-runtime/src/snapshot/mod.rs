//! Approved store: full frames + images, never hash-only baselines.
//!
//! Layout under a store root:
//! ```text
//! approved/<name>.frame.json   approved/<name>.png
//! actual/<name>.frame.json     actual/<name>.png (+ .png.fidelity.json)
//! diff/<name>.png              report.html
//! ```
//!
//! Rules (failure handling is part of the design):
//! - actual artifacts are written BEFORE any assertion — a failing test
//!   still leaves reviewable evidence;
//! - approved artifacts are preserved untouched by `check` (only explicit
//!   [`Store::accept`] replaces them);
//! - a mismatch generates a visual diff PNG, cell diagnostics, and an HTML
//!   report; the gate then fails with artifact paths, not a bare hash;
//! - missing approval fails closed ("new snapshot requires review");
//! - corrupt approval files are explicit errors, never silent defaults;
//! - acceptance is an explicit local command. There is no env-var
//!   auto-bless: CI must never accept snapshots by itself;
//! - all writes are per-name files via atomic tmp+rename, so parallel test
//!   processes updating different names are safe (the index report is
//!   rewritten by whoever finalizes last — data files never clobber).

mod types;
mod paths;
mod check;
mod mutate;
mod report;

pub use types::*;
pub use paths::*;
pub use check::*;
pub use mutate::*;
pub use report::*;
