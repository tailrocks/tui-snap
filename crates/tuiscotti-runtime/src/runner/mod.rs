//! Runner-neutral test context + cargo-nextest adapter (backlog N02–N10).
//!
//! This module never assumes which runner executes the test. Under
//! cargo-nextest it derives stable identity from the environment; under plain
//! `cargo test` (or any other runner) it degrades to a local run identity with
//! attempt `0`. It performs no global mutation: no `set_var`, no `set_current_dir`.
//!
//! # Qualified nextest surface
//!
//! Verified against installed `cargo-nextest 0.9.143` plus
//! `https://nexte.st/docs/configuration/env-vars/` and
//! `https://nexte.st/docs/glossary/`:
//!
//! | Variable | Since | Meaning |
//! |---|---|---|
//! | `NEXTEST_RUN_ID` | 0.9.138 | UUID shared by one `cargo nextest run` invocation |
//! | `NEXTEST_BINARY_ID` | 0.9.116 | `crate` \| `crate::bin` \| `crate::kind/bin` |
//! | `NEXTEST_TEST_NAME` | 0.9.116 | Test name |
//! | `NEXTEST_ATTEMPT` | 0.9.116 | 1-indexed attempt number (`"1"` without retries) |
//! | `NEXTEST_TOTAL_ATTEMPTS` | 0.9.116 | Configured attempt count |
//! | `NEXTEST_ATTEMPT_ID` | 0.9.116 | Globally unique per-attempt id (contains `$`) |
//! | `NEXTEST_STRESS_CURRENT` | 0.9.116 | 0-indexed stress index, or `"none"` |
//! | `NEXTEST_STRESS_TOTAL` | 0.9.116 | Stress total, `"unknown"`, or `"none"` |
//! | `NEXTEST_PROFILE` | 0.9.89 | Nextest profile in use |
//! | `NEXTEST_VERSION` | 0.9.130 | Nextest semver string |
//! | `NEXTEST_WORKSPACE_ROOT` | 0.9.130 | Workspace root (remap-aware) |
//! | `NEXTEST_BIN_EXE_<name>` | 0.9.113 | Remapped binary path; hyphen and underscore forms |
//! | `NEXTEST_EXECUTION_MODE` | — | Currently always `process-per-test` |
//! | `NEXTEST_TEST_GROUP` (+`_SLOT`, `_GLOBAL_SLOT`) | 0.9.90 | Test-group placement |
//!
//! There are **no** `NEXTEST_SHARD_*` variables: partitioning is a run-time
//! selection, not per-test identity. [`AttemptId::shard`] is therefore always
//! `None` from the environment; callers that know their partition set it with
//! [`AttemptId::with_shard`]. (Live probe on 0.9.143 also shows
//! `NEXTEST_RUN_MODE`, `NEXTEST_TEST_PHASE`, and
//! `NEXTEST_{REQUIRED,RECOMMENDED}_VERSION`, which this adapter does not consume.)
//!
//! Nextest ≥ 0.9.116 is required for attempt identity; run correlation
//! (`NEXTEST_RUN_ID`) needs ≥ 0.9.138. Older runners degrade to local identity.

mod context;
mod ids;
mod journal;
mod junit;
mod manifest;
mod resolve;

pub use context::*;
pub use ids::*;
pub use journal::*;
pub use junit::*;
pub use manifest::*;
pub use resolve::*;
