//! tuiscotti-fixtures: shared fixture apps and committed approvals.
//!
//! The [`views`] models/renderers double as the PTY target and the
//! headless-test source; `tests/fixtures`, `tests/visual`, and
//! `tests/snapshots` carry the byte-identical committed approvals.

pub mod driver;
pub mod views;
