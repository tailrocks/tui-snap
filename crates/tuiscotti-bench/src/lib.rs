//! `tuiscotti-bench`: committed micro-benchmark harness (goal §7).
//!
//! Two binaries emit raw per-sample JSONL consumed by `cargo xtask bench`:
//! `bench_views` (in-process capture/render/compare) and `bench_pty` (live
//! PTY fixtures). Nothing here gates correctness; numbers are evidence.

pub mod driver;
pub mod emit;
pub mod fixtures;
pub mod pty;
pub mod rss;
pub mod views;
pub mod views_cache;
