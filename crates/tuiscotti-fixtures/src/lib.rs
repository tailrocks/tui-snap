//! tuiscotti-fixtures: shared fixture app and committed approvals.
//!
//! The [`fixture_app`] model/view doubles as the PTY target and the
//! headless-test source; `tests/fixtures`, `tests/visual`, and
//! `tests/snapshots` carry the byte-identical committed approvals.

pub mod fixture_app;
