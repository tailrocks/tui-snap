//! Shared bench driver: CLI args, `Instant` measurement, sample assembly.

use crate::emit::Sample;
use crate::rss;
use std::path::PathBuf;
use std::time::Instant;

/// Parsed harness arguments (superset for both binaries).
#[derive(Debug)]
pub struct Args {
    /// Wanted scenarios (`all` or comma names).
    pub scenarios: Vec<String>,
    /// Wanted size indexes into [`SIZES`](crate::fixtures::SIZES).
    pub sizes: Vec<usize>,
    /// Per-journey canonical / sweep-total / PTY samples.
    pub samples: u32,
    /// Per-journey full samples.
    pub full: u32,
    /// Per-case compare samples.
    pub cmp: u32,
    /// Per-state cache samples.
    pub cache_n: u32,
    /// Sweep workers.
    pub workers: u32,
    /// JSONL output path.
    pub out: PathBuf,
}

/// `--help`/`-h` request: returned as an error so the library never
/// process-exits; binaries downcast to [`HelpText`] to print + exit(0).
#[derive(Debug)]
pub struct HelpText(pub String);

impl std::fmt::Display for HelpText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for HelpText {}

/// Parse harness arguments.
///
/// # Errors
///
/// Returns an error when flags are malformed or `--out` is missing.
/// `--help`/`-h` returns a [`HelpText`] error (downcast at the binary
/// boundary to print the text and exit 0) instead of process-exiting.
pub fn parse_args(argv: &[String], help: &str, samples: u32) -> anyhow::Result<Args> {
    let mut scenarios = vec!["all".to_string()];
    let mut sizes = vec!["all".to_string()];
    let mut parsed = Args {
        scenarios: Vec::new(),
        sizes: Vec::new(),
        samples,
        full: 10,
        cmp: 60,
        cache_n: 20,
        workers: 4,
        out: PathBuf::new(),
    };
    let mut out: Option<PathBuf> = None;
    let mut i = 1;
    while i < argv.len() {
        let key = argv[i].as_str();
        if key == "--help" || key == "-h" {
            return Err(anyhow::Error::new(HelpText(help.to_string())));
        }
        if key == "--quick" {
            parsed.samples = samples / 4;
            parsed.full = 3;
            parsed.cmp = 12;
            parsed.cache_n = 6;
            i += 1;
            continue;
        }
        if i + 1 >= argv.len() {
            return Err(anyhow::anyhow!("{key} needs a value\n{help}"));
        }
        let val = argv[i + 1].as_str();
        match key {
            "--scenario" => scenarios = val.split(',').map(str::to_string).collect(),
            "--size" => sizes = val.split(',').map(str::to_string).collect(),
            "--samples" => parse_into(val, "--samples", &mut parsed.samples)?,
            "--full" => parse_into(val, "--full", &mut parsed.full)?,
            "--cmp" => parse_into(val, "--cmp", &mut parsed.cmp)?,
            "--cache-n" => parse_into(val, "--cache-n", &mut parsed.cache_n)?,
            "--workers" => parse_into(val, "--workers", &mut parsed.workers)?,
            "--out" => out = Some(PathBuf::from(val)),
            _ => return Err(anyhow::anyhow!("unknown arg: {key}\n{help}")),
        }
        i += 2;
    }
    parsed.scenarios = scenarios;
    parsed.out = out.ok_or_else(|| anyhow::anyhow!("missing --out\n{help}"))?;
    for (k, s) in crate::fixtures::SIZES.iter().enumerate() {
        if sizes.contains(&"all".to_string()) || sizes.contains(&s.2.to_string()) {
            parsed.sizes.push(k);
        }
    }
    if parsed.sizes.is_empty() {
        return Err(anyhow::anyhow!("no matching --size"));
    }
    Ok(parsed)
}

///
/// # Errors
///
/// Returns an error when the operation fails.
fn parse_into(val: &str, name: &str, slot: &mut u32) -> anyhow::Result<()> {
    *slot = val.parse().map_err(|_| anyhow::anyhow!("bad {name}"))?;
    Ok(())
}

/// True when scenario `name` was requested.
#[must_use]
pub fn wants(args: &Args, name: &str) -> bool {
    args.scenarios.iter().any(|s| s == "all" || s == name)
}

/// One measurement: wall time + sampled peak RSS.
#[derive(Debug)]
pub struct Meas {
    /// Wall time, nanoseconds.
    pub elapsed_ns: u128,
    /// Sampled process peak RSS (0 when unavailable).
    pub rss: u64,
    /// RSS unit.
    pub rss_units: &'static str,
}

/// Time `op` with `Instant`, sampling peak RSS after the clock stops so
/// sampling overhead stays outside the timed region.
pub fn measure(op: impl FnOnce() -> (bool, String)) -> (Meas, bool, String) {
    let start = Instant::now();
    let (ok, detail) = op();
    let elapsed = start.elapsed().as_nanos();
    let (rss, rss_units) = rss::peak_rss().unwrap_or((0, "unknown"));
    (
        Meas {
            elapsed_ns: elapsed,
            rss,
            rss_units,
        },
        ok,
        detail,
    )
}

/// Blank sample for `suite`/`scenario`.
#[must_use]
pub fn base(
    suite: &'static str,
    scenario: &'static str,
    size: &'static str,
    journey: &'static str,
) -> Sample {
    Sample {
        suite,
        scenario,
        size,
        journey,
        case: "-",
        cache: "-",
        worker: 0,
        iter: 0,
        elapsed_ns: 0,
        rss: 0,
        rss_units: "unknown",
        ok: false,
        detail: String::new(),
    }
}

/// Record a measurement into a sample.
pub fn fill(sample: &mut Sample, meas: &Meas, ok: bool, detail: String) {
    sample.elapsed_ns = meas.elapsed_ns;
    sample.rss = meas.rss;
    sample.rss_units = meas.rss_units;
    sample.ok = ok;
    sample.detail = detail;
}
