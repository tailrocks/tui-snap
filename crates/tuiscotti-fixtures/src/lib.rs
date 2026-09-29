//! tuiscotti-fixtures: shared fixture apps and committed approvals.
//!
//! The [`views`] models/renderers double as the PTY target and the
//! headless-test source; `tests/fixtures`, `tests/visual`, and
//! `tests/snapshots` carry the byte-identical committed approvals.
//! [`fixture_app`] is the legacy matrix app kept for the existing visual
//! gates; new contracts use [`views`] + the `*_fixture` binaries.

pub mod driver;
pub mod fixture_app;
pub mod views;
