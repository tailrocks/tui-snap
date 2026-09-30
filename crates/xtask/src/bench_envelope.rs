//! Benchmark envelope: repro metadata + scoreboard JSON writer.

use std::path::Path;

use crate::bench_score::{Args, Gate, Sample, select, stats_of};
use crate::sha256;
use crate::util::{self, Result};

pub(crate) fn esc(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

/// Join pre-rendered item lines into a JSON array body (comma-separated,
/// trailing newline when non-empty).
pub(crate) fn array(items: &[String]) -> String {
    if items.is_empty() {
        String::new()
    } else {
        items.join(",\n") + "\n"
    }
}

pub(crate) fn prog(root: &Path, name: &str, args: &[&str]) -> String {
    let exe = std::ffi::OsString::from(name);
    util::run_program(root, &exe, args)
        .unwrap_or_else(|_| "unknown".to_string())
        .trim()
        .to_string()
}

pub(crate) fn corpus_sha(root: &Path) -> String {
    let files = util::walk_files(root).unwrap_or_default();
    let mut bytes = Vec::new();
    for path in files {
        let rel = util::display(root, &path);
        let wanted = rel.starts_with("crates/tuiscotti-bench/")
            || rel.starts_with("crates/tuiscotti-fixtures/src/")
            || rel == "Cargo.lock";
        if wanted && let Ok(content) = std::fs::read(&path) {
            bytes.extend_from_slice(rel.as_bytes());
            bytes.extend_from_slice(&content);
        }
    }
    sha256::hexdigest(&bytes)
}

/// Scored run data for the envelope.
pub(crate) struct Report<'a> {
    pub(crate) stamp: u64,
    pub(crate) samples: &'a [Sample],
    pub(crate) walls: &'a [(String, f64)],
    pub(crate) gates: &'a [Gate],
    pub(crate) notes: &'a [String],
}

pub(crate) fn write_envelope(
    root: &Path,
    parsed: &Args,
    out: &Path,
    report: &Report<'_>,
) -> Result<()> {
    let mem = if cfg!(target_os = "macos") {
        prog(root, "sysctl", &["-n", "hw.memsize"])
    } else {
        prog(
            root,
            "sh",
            &["-c", "grep MemTotal /proc/meminfo || echo unknown"],
        )
    };
    let budgets = array(
        &report
            .gates
            .iter()
            .map(|g| {
                format!(
                    "    {{\"id\":\"{}\",\"target\":\"{}\",\"observed\":\"{}\",\"pass\":{}}}",
                    esc(&g.id),
                    esc(&g.target),
                    esc(&g.observed),
                    g.pass
                )
            })
            .collect::<Vec<_>>(),
    );
    let wall_json = array(
        &report
            .walls
            .iter()
            .map(|(k, v)| format!("    {{\"key\":\"{}\",\"ms\":{v:.3}}}", esc(k)))
            .collect::<Vec<_>>(),
    );
    let note_json = array(
        &report
            .notes
            .iter()
            .map(|n| format!("    \"{}\"", esc(n)))
            .collect::<Vec<_>>(),
    );
    let group_json = groups_json(report.samples);
    let cmd = std::env::args().collect::<Vec<_>>().join(" ");
    let stamp = report.stamp;
    let doc = format!(
        "{{\n  \"tool\": \"cargo xtask bench\",\n  \"command\": \"{}\",\n  \"stamp\": {stamp},\n  \"quick\": {},\n  \"suite\": \"{}\",\n  \"source_sha\": \"{}\",\n  \"dirty_files\": {},\n  \"corpus_sha256\": \"{}\",\n  \"rustc\": \"{}\",\n  \"cargo\": \"{}\",\n  \"nextest\": \"{}\",\n  \"profile\": \"release\",\n  \"os\": \"{}\",\n  \"cpus\": {},\n  \"mem\": \"{}\",\n  \"cache_states\": \"warm-shared/fresh/empty/populated/no-cache (bins record per-sample cache field)\",\n  \"concurrency\": \"views+pty sweeps 1/2/4/8/16 (+32 views oversub); nextest -j sweeps\",\n  \"limits\": \"default worker/queue bounds; flood capped at seq 200000; paste 1MiB\",\n  \"samples\": {},\n  \"budgets\": [\n{budgets}  ],\n  \"walls_ms\": [\n{wall_json}  ],\n  \"groups\": [\n{group_json}  ],\n  \"notes\": [\n{note_json}  ]\n}}\n",
        esc(&cmd),
        parsed.quick,
        esc(&parsed.suite),
        esc(prog(root, "git", &["rev-parse", "HEAD"])
            .lines()
            .next()
            .unwrap_or("unknown")),
        prog(root, "git", &["status", "--short"]).lines().count(),
        corpus_sha(root),
        esc(prog(root, "rustc", &["--version"])
            .lines()
            .next()
            .unwrap_or("unknown")),
        esc(prog(root, "cargo", &["--version"])
            .lines()
            .next()
            .unwrap_or("unknown")),
        esc(prog(root, "cargo", &["nextest", "--version"])
            .lines()
            .next()
            .unwrap_or("unknown")),
        esc(prog(root, "uname", &["-sm"])
            .lines()
            .next()
            .unwrap_or("unknown")),
        std::thread::available_parallelism().map_or(0, std::num::NonZeroUsize::get),
        esc(&mem),
        report.samples.len(),
    );
    let path = out.join(format!("{}-envelope.json", report.stamp));
    std::fs::write(&path, doc).map_err(|e| util::fail(format!("write {}: {e}", path.display())))?;
    println!("bench: wrote {}", util::display(root, &path));
    Ok(())
}

pub(crate) fn groups_json(samples: &[Sample]) -> String {
    let mut keys: Vec<(String, String)> = Vec::new();
    for s in samples {
        if !keys.iter().any(|(a, b)| a == &s.scenario && b == &s.size) {
            keys.push((s.scenario.clone(), s.size.clone()));
        }
    }
    keys.sort();
    array(
        &keys
            .iter()
            .map(|(scenario, size)| {
                let (mut v, fail) = select(samples, scenario, size);
                let st = stats_of(&mut v, fail);
                let mut worst = ("-", "-", 0.0);
                for s in samples {
                    if &s.scenario == scenario && &s.size == size && s.ok && s.elapsed_ns >= worst.2
                    {
                        worst = (s.journey.as_str(), s.case.as_str(), s.elapsed_ns);
                    }
                }
                format!(
                    "    {{\"key\":\"{} {}\",\"n\":{},\"mean_ms\":{:.3},\"p50_ms\":{:.3},\"p95_ms\":{:.3},\"max_ms\":{:.3},\"fail\":{},\"worst\":\"{} {}\"}}",
                    esc(scenario),
                    esc(size),
                    st.n,
                    st.mean_ms,
                    st.p50_ms,
                    st.p95_ms,
                    st.max_ms,
                    st.fail,
                    esc(worst.0),
                    esc(worst.1)
                )
            })
            .collect::<Vec<_>>(),
    )
}

pub(crate) fn print_scoreboard(gates: &[Gate]) {
    println!("scoreboard:");
    for g in gates {
        println!(
            "  {} {}: {} [{}]",
            if g.pass { "PASS" } else { "FAIL" },
            g.id,
            g.observed,
            g.target
        );
    }
}
