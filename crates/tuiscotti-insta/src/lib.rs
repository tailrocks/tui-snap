//! tuiscotti-insta: snapshot/screenshot assertion facade over Insta.
//!
//! [`assert_snapshot!`] and [`assert_screenshot!`] expand Insta assertions at
//! the CALLER, plus the frozen-policy helpers ([`assert`]) and the compound
//! canonical-plus-PNG snapshot lifecycle ([`insta_proto`]).
//!
//! Only public Insta APIs are used; there is no Insta fork or private clone.

pub mod assert;
pub mod insta_proto;

/// Re-exported for macro expansion: the facade macros expand
/// `$crate::insta::assert_snapshot!` at the caller, so consumers need no
/// direct `insta` dependency for metadata to resolve there.
pub use insta;
