//! Benchmark probes: fresh-CLI timing, build matrix, nextest runs.

use std::path::Path;
use std::time::Instant;

use crate::bench::bin;
use crate::bench::unix_secs;
use crate::bench_score::Args;
use crate::util::{self, Result};

pub(crate) fn run_cli(
    root: &Path,
    parsed: &Args,
    walls: &mut Vec<(String, f64)>,
    notes: &mut Vec<String>,
) -> Result<()> {
    let exe = bin(root, "tuiscotti");
    let count = if parsed.quick { 8_u32 } else { 25 };
    // Recorded warm-up: the first exec of a fresh binary pays OS/code-cache
    // costs (notably macOS first-run verification); the gate scores warm
    // fresh-process runs, with the cold sample kept as `cli-warmup`.
    let warm_dir = std::env::temp_dir().join(format!("bench-cli-{}-warmup", unix_secs()));
    let warm_start = Instant::now();
    let warm_status = std::process::Command::new(&exe)
        .args(["capture", "--out"])
        .arg(&warm_dir)
        .args(["--", "/bin/echo", "hello"])
        .output()
        .map(|o| o.status)?;
    walls.push((
        "cli-warmup".to_string(),
        warm_start.elapsed().as_secs_f64() * 1000.0,
    ));
    notes.push(format!("cli warmup exit_ok={}", warm_status.success()));
    drop(std::fs::remove_dir_all(&warm_dir));
    for i in 0..count {
        let dir = std::env::temp_dir().join(format!("bench-cli-{}-{i}", unix_secs()));
        let start = Instant::now();
        let out = std::process::Command::new(&exe)
            .args(["capture", "--out"])
            .arg(&dir)
            .args(["--", "/bin/echo", "hello"])
            .output()?;
        let status = out.status;
        let ms = start.elapsed().as_secs_f64() * 1000.0;
        walls.push((format!("g3-cli-p95 #{i}"), ms));
        if !status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            let line = err
                .lines()
                .next()
                .unwrap_or("")
                .chars()
                .take(120)
                .collect::<String>();
            notes.push(format!(
                "cli sample {i} exited {}: {line}",
                status.code().unwrap_or(-1)
            ));
        }
        drop(std::fs::remove_dir_all(&dir));
    }
    notes.push("cli: warm filesystem cache assumed; drop_caches not run".to_string());
    Ok(())
}

pub(crate) fn touch_same(path: &Path) -> Result<()> {
    let bytes =
        std::fs::read(path).map_err(|e| util::fail(format!("read {}: {e}", path.display())))?;
    std::fs::write(path, bytes)
        .map_err(|e| util::fail(format!("touch {}: {e}", path.display())))?;
    Ok(())
}

pub(crate) fn timed_cargo(root: &Path, args: &[&str]) -> (f64, bool) {
    let start = Instant::now();
    let ok = util::run_cargo(root, args).is_ok();
    (start.elapsed().as_secs_f64() * 1000.0, ok)
}

/// Like [`timed_cargo`] but WITHOUT the xtask recursion guard: test runners
/// (cargo test / nextest) spawn test binaries that may legitimately invoke
/// `xtask` helpers themselves (e.g. xtask's own CLI tests). Inheriting the
/// guard would make those fail with "refusing recursive invocation".
/// No test invokes `bench`, so no recursion cycle can form.
pub(crate) fn timed_cargo_tests(root: &Path, args: &[&str]) -> (f64, bool) {
    let cargo: std::ffi::OsString =
        std::env::var_os("CARGO").unwrap_or_else(|| std::ffi::OsString::from("cargo"));
    let start = Instant::now();
    let ok = std::process::Command::new(cargo)
        .args(args)
        .current_dir(root)
        .env_remove(util::GUARD_ENV)
        .output()
        .is_ok_and(|o| o.status.success());
    (start.elapsed().as_secs_f64() * 1000.0, ok)
}

pub(crate) fn run_builds(
    root: &Path,
    parsed: &Args,
    walls: &mut Vec<(String, f64)>,
    notes: &mut Vec<String>,
) -> Result<()> {
    drop(util::run_cargo(
        root,
        &["build", "--offline", "--workspace"],
    ));
    for i in 0..3 {
        let (ms, _) = timed_cargo(root, &["build", "--offline", "--workspace"]);
        walls.push((format!("warm-noop #{i}"), ms));
    }
    if !parsed.quick {
        let scratch = std::env::temp_dir().join(format!("bench-target-{}", unix_secs()));
        let cargo: std::ffi::OsString =
            std::env::var_os("CARGO").unwrap_or_else(|| std::ffi::OsString::from("cargo"));
        let start = Instant::now();
        let out = std::process::Command::new(cargo)
            .args(["build", "--offline", "--workspace"])
            .current_dir(root)
            .env("CARGO_TARGET_DIR", &scratch)
            .env(util::GUARD_ENV, "1")
            .output()?;
        let status = out.status;
        walls.push((
            "target-clean-scratch".to_string(),
            start.elapsed().as_secs_f64() * 1000.0,
        ));
        notes.push(format!(
            "scratch target {} success={}",
            scratch.display(),
            status.success()
        ));
        drop(std::fs::remove_dir_all(&scratch));
    }
    let edits = [
        (
            "g7-core-edit",
            "crates/tuiscotti-core/src/screen/grid.rs",
            vec!["test", "--offline", "-p", "tuiscotti-render", "--lib"],
        ),
        (
            "g7-render-edit",
            "crates/tuiscotti-render/src/render/renderer.rs",
            vec!["test", "--offline", "-p", "tuiscotti-render", "--lib"],
        ),
        (
            "g7-view-edit",
            "crates/tuiscotti-fixtures/src/views/menu.rs",
            vec![
                "test",
                "--offline",
                "-p",
                "tuiscotti-fixtures",
                "--test",
                "view_contracts",
            ],
        ),
    ];
    // Warm the exact test-profile fingerprints g7 times: the `cargo build`
    // warmups above do not cover `cargo test` targets, so the first timed
    // edit otherwise pays a cold-fingerprint rebuild (measured 10x noise).
    for (id, _, cmd) in &edits {
        let mut warm = cmd.clone();
        warm.push("--no-run");
        let (_, ok) = timed_cargo_tests(root, &warm);
        notes.push(format!("{id} warmup exit_ok={ok}"));
    }
    for (id, file, cmd) in edits {
        // Median of 3 touches: single edit-to-verdict samples are noisy
        // (measured 0.4–10 s for the identical command across cache
        // states), so the gate scores the median, never one sample.
        let mut samples = Vec::with_capacity(3);
        let mut ok_all = true;
        for _ in 0..3 {
            touch_same(&root.join(file))?;
            let (ms, ok) = timed_cargo_tests(root, &cmd);
            samples.push(ms);
            ok_all &= ok;
        }
        samples.sort_by(f64::total_cmp);
        walls.push((format!("{id} Ms"), samples[1]));
        walls.push((format!("{id} exit-ok"), if ok_all { 1.0 } else { 0.0 }));
        notes.push(format!(
            "{id}: {} samples_ms={samples:?} exit_ok={ok_all}",
            cmd.join(" ")
        ));
    }
    Ok(())
}

pub(crate) fn run_nextest(
    root: &Path,
    parsed: &Args,
    out: &Path,
    stamp: u64,
    walls: &mut Vec<(String, f64)>,
    notes: &mut Vec<String>,
) {
    let cargo: std::ffi::OsString =
        std::env::var_os("CARGO").unwrap_or_else(|| std::ffi::OsString::from("cargo"));
    let start = Instant::now();
    let full = std::process::Command::new(cargo)
        .args(["nextest", "run", "--offline", "--all-features"])
        .current_dir(root)
        .env_remove(util::GUARD_ENV)
        .output();
    let ms = start.elapsed().as_secs_f64() * 1000.0;
    let ok = full.as_ref().is_ok_and(|o| o.status.success());
    walls.push(("g8-nextest-full Ms".to_string(), ms));
    walls.push((
        "g8-nextest-full exit-ok".to_string(),
        if ok { 1.0 } else { 0.0 },
    ));
    match full {
        Ok(o) => {
            let text = format!(
                "--- stdout ---\n{}\n--- stderr ---\n{}",
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            );
            drop(std::fs::write(
                out.join(format!("{stamp}-nextest-full.log")),
                &text,
            ));
            let summary = text
                .lines()
                .find(|l| l.contains("tests run:"))
                .unwrap_or("no nextest summary line")
                .chars()
                .take(200)
                .collect::<String>();
            notes.push(format!("nextest full exit_ok={ok} summary={summary}"));
        }
        Err(e) => notes.push(format!("nextest full failed to launch: {e}")),
    }
    let workers = if parsed.quick {
        vec!["1", "4"]
    } else {
        vec!["1", "2", "4", "8", "16"]
    };
    for w in workers {
        let (ms, _) = timed_cargo_tests(
            root,
            &[
                "nextest",
                "run",
                "--offline",
                "-p",
                "tuiscotti",
                "--test",
                "render_qual",
                "--test-threads",
                w,
            ],
        );
        walls.push((format!("nextest-render_qual-j{w}"), ms));
    }
    for w in ["1", "4"] {
        let (ms, _) = timed_cargo_tests(
            root,
            &[
                "nextest",
                "run",
                "--offline",
                "-p",
                "tuiscotti",
                "--test",
                "tui",
                "--test-threads",
                w,
            ],
        );
        walls.push((format!("nextest-tui-j{w}"), ms));
    }
}
