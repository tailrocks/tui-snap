//! Integration tests for the runner-neutral context + nextest adapter.
//!
//! These tests never mutate the process environment or CWD (parallel-safe
//! under both libtest and nextest); all env-driven behavior goes through the
//! `*_with_map` / `from_map` constructors with injected maps.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[path = "runner/env.rs"]
mod env;
#[path = "runner/isolation.rs"]
mod isolation;
#[path = "runner/journal.rs"]
mod journal;

fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn nextest_env(root: &Path) -> Result<HashMap<String, String>, Box<dyn std::error::Error>> {
    let workspace = root.to_str().ok_or("utf8 tmp path")?;
    Ok(env(&[
        ("NEXTEST_RUN_ID", "1f79aa0d-4ec8-4a5c-aa83-5e8dc2f36573"),
        ("NEXTEST_BINARY_ID", "tuiscotti::runner"),
        ("NEXTEST_TEST_NAME", "env_parsing"),
        ("NEXTEST_ATTEMPT", "2"),
        ("NEXTEST_TOTAL_ATTEMPTS", "3"),
        ("NEXTEST_ATTEMPT_ID", "1f79$abc"),
        ("NEXTEST_STRESS_CURRENT", "none"),
        ("NEXTEST_STRESS_TOTAL", "none"),
        ("NEXTEST_PROFILE", "ci"),
        ("NEXTEST_VERSION", "0.9.143"),
        ("NEXTEST_WORKSPACE_ROOT", workspace),
        ("NEXTEST_EXECUTION_MODE", "process-per-test"),
    ]))
}

fn tmp_root(tag: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let p = std::env::temp_dir().join(format!(
        "tuisnap-runner-{}-{}-{}",
        tag,
        std::process::id(),
        // nanos make parallel nextest processes distinct
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    fs::create_dir_all(&p)?;
    Ok(p)
}
