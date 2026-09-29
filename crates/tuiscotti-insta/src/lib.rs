//! tuiscotti-insta: snapshot/screenshot assertion facade over Insta.
//!
//! [`assert_snapshot!`] and [`assert_screenshot!`] plus the frozen-policy
//! helpers ([`assert`]) and the compound canonical-plus-PNG snapshot
//! lifecycle ([`insta_proto`]).

pub mod assert;
pub mod insta_proto;
