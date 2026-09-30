//! Committed reproducible benchmark suite (goal §7).
//!
//! Drives the `tuiscotti-bench` binaries plus CLI/build/nextest probes,
//! collects raw per-sample JSONL, and scores the 8 acceptance budgets.
//! Zero-dependency: hand-rolled stats and JSON.

use std::path::{Path, PathBuf};

use crate::bench_envelope::{self, Report};
use crate::bench_probe;
use crate::bench_score::{self, Args, Gate, Sample};
use crate::util::{self, Result, Status};

/// Subcommand name.
pub const NAME: &str = "bench";

/// Subcommand usage.
pub const HELP: &str = "\
usage: cargo xtask bench [--quick] [--suite views|pty|cli|builds|nextest|all] [--out DIR]\n\
\n\
runs the reproducible benchmark suite at the current head:\n\
  views    canonical/full/compare/cache micro-bench + worker sweep\n\
  pty      readiness/journey/cleanup + PTY worker sweep\n\
  cli      fresh release CLI capture processes\n\
  builds   warm no-op, scratch-target clean, edit-to-verdict\n\
  nextest  full warm run + worker sweeps\n\
writes benches/results/<stamp>-*.jsonl (raw samples) + <stamp>-envelope.json\n\
(repro data + budget scoreboard). Exit 1 when any budget fails.\n";

/// Run the benchmark suite.
///
/// # Errors
///
/// Returns an error on bad flags, failed builds/probes, or unwritable output.
pub fn run(root: &Path, args: &[String]) -> Result<Status> {
    if util::wants_help(args) {
        println!("{HELP}");
        return Ok(Status::Pass);
    }
    let parsed = parse_args(args)?;
    let out = parsed
        .out
        .clone()
        .unwrap_or_else(|| root.join("benches").join("results"));
    std::fs::create_dir_all(&out)
        .map_err(|e| util::fail(format!("create {}: {e}", out.display())))?;
    let stamp = unix_secs();
    let want = |name: &str| parsed.suite == "all" || parsed.suite == name;
    let mut gates: Vec<Gate> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    notes.push(
        "rss: Linux-only VmHWM from /proc/self/status (kibibytes); rss=0/rss_units=unknown elsewhere"
            .to_string(),
    );
    let mut samples: Vec<Sample> = Vec::new();
    let mut walls: Vec<(String, f64)> = Vec::new();
    if want("views") || want("pty") {
        println!("bench: building release harness...");
        build_bins(root)?;
    }
    if want("views") {
        println!("bench: views micro-bench + sweep...");
        run_views(root, &parsed, &out, stamp, &mut samples, &mut walls)?;
        bench_score::score_views(&samples, &walls, &mut gates);
    }
    if want("pty") {
        println!("bench: pty fixtures + sweep...");
        run_pty(root, &parsed, &out, stamp, &mut samples, &mut walls)?;
        bench_score::score_pty(&samples, &mut gates);
    }
    if want("cli") {
        println!("bench: fresh CLI processes...");
        bench_probe::run_cli(root, &parsed, &mut walls, &mut notes)?;
        bench_score::score_wall(
            &walls,
            "g3-cli-p95",
            "fresh CLI p95 <= 250 ms",
            250.0,
            &mut gates,
        );
    }
    if want("builds") {
        println!("bench: build matrix + edit-to-verdict...");
        bench_probe::run_builds(root, &parsed, &mut walls, &mut notes)?;
        bench_score::score_builds(&walls, &mut gates);
    }
    if want("nextest") {
        println!("bench: nextest full + sweeps...");
        bench_probe::run_nextest(root, &parsed, &out, stamp, &mut walls, &mut notes);
        bench_score::score_nextest_full(&walls, &mut gates);
    }
    bench_envelope::write_envelope(
        root,
        &parsed,
        &out,
        &Report {
            stamp,
            samples: &samples,
            walls: &walls,
            gates: &gates,
            notes: &notes,
        },
    )?;
    bench_envelope::print_scoreboard(&gates);
    let pass = gates.iter().all(|g| g.pass);
    Ok(if pass { Status::Pass } else { Status::Fail })
}

fn parse_args(args: &[String]) -> Result<Args> {
    let mut parsed = Args {
        quick: false,
        suite: "all".to_string(),
        out: None,
    };
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--quick" => parsed.quick = true,
            "--suite" => {
                i += 1;
                parsed.suite = args.get(i).cloned().unwrap_or_default();
            }
            "--out" => {
                i += 1;
                parsed.out = args.get(i).map(PathBuf::from);
            }
            other => return Err(util::fail(format!("{NAME}: unexpected arg: {other}"))),
        }
        i += 1;
    }
    if !["all", "views", "pty", "cli", "builds", "nextest"].contains(&parsed.suite.as_str()) {
        return Err(util::fail(format!("{NAME}: bad --suite: {}", parsed.suite)));
    }
    Ok(parsed)
}

pub(crate) fn unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn build_bins(root: &Path) -> Result<()> {
    util::run_cargo(
        root,
        &["build", "--offline", "-p", "tuiscotti-bench", "--release"],
    )?;
    util::run_cargo(
        root,
        &["build", "--offline", "-p", "tuiscotti-cli", "--release"],
    )?;
    Ok(())
}

pub(crate) fn bin(root: &Path, name: &str) -> PathBuf {
    root.join("target").join("release").join(name)
}

fn run_bin(root: &Path, name: &str, args: &[&str]) -> Result<String> {
    let exe = bin(root, name).into_os_string();
    util::run_program(root, &exe, args)
}

fn out_path(out: &Path, stamp: u64, name: &str) -> PathBuf {
    out.join(format!("{stamp}-{name}.jsonl"))
}

fn run_main(root: &Path, bin: &str, file: &Path, scenarios: &str, quick: bool) -> Result<()> {
    let path = file.to_string_lossy().to_string();
    let mut v = vec!["--out", path.as_str(), "--scenario", scenarios];
    if quick {
        v.push("--quick");
    }
    run_bin(root, bin, &v)?;
    Ok(())
}

fn run_sweep(
    root: &Path,
    bin: &str,
    file: &Path,
    worker: u32,
    samples: &str,
    quick: bool,
) -> Result<Option<f64>> {
    let path = file.to_string_lossy().to_string();
    let w = worker.to_string();
    let mut v = vec![
        "--out",
        path.as_str(),
        "--scenario",
        "sweep",
        "--workers",
        &w,
        "--samples",
        samples,
    ];
    if quick {
        v.push("--quick");
    }
    Ok(bench_score::parse_wall(&run_bin(root, bin, &v)?))
}

fn run_views(
    root: &Path,
    parsed: &Args,
    out: &Path,
    stamp: u64,
    samples: &mut Vec<Sample>,
    walls: &mut Vec<(String, f64)>,
) -> Result<()> {
    let main = out_path(out, stamp, "views");
    run_main(
        root,
        "bench_views",
        &main,
        "canonical,full,compare,cached",
        parsed.quick,
    )?;
    bench_score::load_jsonl(&main, samples)?;
    let counts = if parsed.quick { "16" } else { "64" };
    let workers: &[u32] = if parsed.quick {
        &[1, 4]
    } else {
        &[1, 2, 4, 8, 16, 32]
    };
    for w in workers {
        let file = out.join(format!("{stamp}-views-sweep-{w}.jsonl"));
        if let Some(wall) = run_sweep(root, "bench_views", &file, *w, counts, parsed.quick)? {
            walls.push((format!("views-sweep-{w}"), wall));
        }
        bench_score::load_jsonl(&file, samples)?;
    }
    Ok(())
}

fn run_pty(
    root: &Path,
    parsed: &Args,
    out: &Path,
    stamp: u64,
    samples: &mut Vec<Sample>,
    walls: &mut Vec<(String, f64)>,
) -> Result<()> {
    let main = out_path(out, stamp, "pty");
    run_main(
        root,
        "bench_pty",
        &main,
        "readiness,journey,cleanup",
        parsed.quick,
    )?;
    bench_score::load_jsonl(&main, samples)?;
    let workers: &[u32] = if parsed.quick { &[1, 4] } else { &[1, 2, 4, 8] };
    for w in workers {
        let file = out.join(format!("{stamp}-pty-sweep-{w}.jsonl"));
        if let Some(wall) = run_sweep(root, "bench_pty", &file, *w, "16", parsed.quick)? {
            walls.push((format!("pty-sweep-{w}"), wall));
        }
        bench_score::load_jsonl(&file, samples)?;
    }
    Ok(())
}
