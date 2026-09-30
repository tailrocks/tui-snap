//! Benchmark scoring: JSONL parsing, hand-rolled stats, budget gates, envelopes.

use std::path::{Path, PathBuf};

use crate::util::{self, Result};

pub(crate) struct Args {
    pub(crate) quick: bool,
    pub(crate) suite: String,
    pub(crate) out: Option<PathBuf>,
}

pub(crate) struct Sample {
    pub(crate) scenario: String,
    pub(crate) size: String,
    pub(crate) journey: String,
    pub(crate) case: String,
    pub(crate) ok: bool,
    pub(crate) elapsed_ns: f64,
}

pub(crate) struct Stats {
    pub(crate) n: usize,
    pub(crate) mean_ms: f64,
    pub(crate) p50_ms: f64,
    pub(crate) p95_ms: f64,
    pub(crate) max_ms: f64,
    pub(crate) fail: usize,
}

pub(crate) struct Gate {
    pub(crate) id: String,
    pub(crate) target: String,
    pub(crate) observed: String,
    pub(crate) pass: bool,
}

pub(crate) fn parse_wall(stdout: &str) -> Option<f64> {
    for token in stdout.split_whitespace() {
        if let Some(ns) = token.strip_prefix("WALL_NS=")
            && let Ok(v) = ns.parse::<f64>()
        {
            return Some(v / 1_000_000.0);
        }
    }
    None
}

pub(crate) fn field_str<'a>(line: &'a str, key: &str) -> &'a str {
    let pat = format!("\"{key}\":\"");
    line.find(&pat).map_or("", |i| {
        let rest = &line[i + pat.len()..];
        rest.find('"').map_or("", |j| &rest[..j])
    })
}

pub(crate) fn field_num(line: &str, key: &str) -> f64 {
    let pat = format!("\"{key}\":");
    line.find(&pat).map_or(0.0, |i| {
        let rest = &line[i + pat.len()..];
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        digits.parse::<f64>().unwrap_or(0.0)
    })
}

pub(crate) fn load_jsonl(path: &Path, samples: &mut Vec<Sample>) -> Result<()> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| util::fail(format!("read {}: {e}", path.display())))?;
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        samples.push(Sample {
            scenario: field_str(line, "scenario").to_string(),
            size: field_str(line, "size").to_string(),
            journey: field_str(line, "journey").to_string(),
            case: field_str(line, "case").to_string(),
            ok: line.contains("\"ok\":true"),
            elapsed_ns: field_num(line, "elapsed_ns"),
        });
    }
    Ok(())
}

pub(crate) fn stats_of(values: &mut [f64], fail: usize) -> Stats {
    values.sort_by(f64::total_cmp);
    let n = values.len();
    let pct = |num: usize| {
        if n == 0 {
            0.0
        } else {
            values[(num.saturating_mul(n).div_ceil(100))
                .saturating_sub(1)
                .min(n - 1)]
        }
    };
    let mean = if n == 0 {
        0.0
    } else {
        values.iter().sum::<f64>() / f64::from(u32::try_from(n).unwrap_or(u32::MAX))
    };
    Stats {
        n,
        mean_ms: mean / 1_000_000.0,
        p50_ms: pct(50) / 1_000_000.0,
        p95_ms: pct(95) / 1_000_000.0,
        max_ms: values.last().copied().unwrap_or(0.0) / 1_000_000.0,
        fail,
    }
}

pub(crate) fn select(samples: &[Sample], scenario: &str, size: &str) -> (Vec<f64>, usize) {
    let mut values = Vec::new();
    let mut fail = 0;
    for s in samples {
        if s.scenario == scenario && (size.is_empty() || s.size == size) {
            if s.ok {
                values.push(s.elapsed_ns);
            } else {
                fail += 1;
            }
        }
    }
    (values, fail)
}

pub(crate) fn score_views(samples: &[Sample], walls: &[(String, f64)], gates: &mut Vec<Gate>) {
    for (size, budget) in [("80x24", 5.0), ("120x40", 10.0), ("200x60", 25.0)] {
        let (mut v, fail) = select(samples, "canonical", size);
        let st = stats_of(&mut v, fail);
        gates.push(Gate {
            id: format!("g1-canonical-{size}"),
            target: format!("canonical p95 <= {budget} ms"),
            observed: format!(
                "p95={:.2} p50={:.2} n={} fail={}",
                st.p95_ms, st.p50_ms, st.n, st.fail
            ),
            pass: st.fail == 0 && st.n > 0 && st.p95_ms <= budget,
        });
        let (mut v, fail) = select(samples, "full", size);
        let full_budget = budget * 10.0;
        let st = stats_of(&mut v, fail);
        gates.push(Gate {
            id: format!("g2-full-{size}"),
            target: format!("full p95 <= {full_budget} ms (fresh render, 0 cache hits)"),
            observed: format!(
                "p95={:.1} p50={:.1} n={} fail={}",
                st.p95_ms, st.p50_ms, st.n, st.fail
            ),
            pass: st.fail == 0 && st.n > 0 && st.p95_ms <= full_budget,
        });
    }
    let wall = |key: &str| {
        walls
            .iter()
            .find(|(k, _)| k == key)
            .map_or(0.0, |(_, v)| *v)
    };
    let (w1, w4) = (wall("views-sweep-1"), wall("views-sweep-4"));
    let ratio = if w4 > 0.0 { w1 / w4 } else { 0.0 };
    gates.push(Gate {
        id: "g6-sweep-x4".to_string(),
        target: "4-worker medium full >= 2.5x single throughput".to_string(),
        observed: format!("{ratio:.2}x (wall1={w1:.0}ms wall4={w4:.0}ms)"),
        pass: ratio >= 2.5,
    });
}

pub(crate) fn score_pty(samples: &[Sample], gates: &mut Vec<Gate>) {
    let (mut v, fail) = select(samples, "readiness", "");
    let st = stats_of(&mut v, fail);
    gates.push(Gate {
        id: "g4a-readiness".to_string(),
        target: "readiness→verdict p95 <= 100 ms".to_string(),
        observed: format!(
            "p95={:.1} p50={:.1} n={} fail={}",
            st.p95_ms, st.p50_ms, st.n, st.fail
        ),
        pass: st.fail == 0 && st.n > 0 && st.p95_ms <= 100.0,
    });
    let (mut v, fail) = select(samples, "journey", "");
    let st = stats_of(&mut v, fail);
    gates.push(Gate {
        id: "g4b-journey".to_string(),
        target: "3-transition journey p95 <= 1000 ms".to_string(),
        observed: format!(
            "p95={:.0} p50={:.0} n={} fail={}",
            st.p95_ms, st.p50_ms, st.n, st.fail
        ),
        pass: st.fail == 0 && st.n > 0 && st.p95_ms <= 1000.0,
    });
    let (mut v, fail) = select(samples, "cleanup", "");
    let st = stats_of(&mut v, fail);
    gates.push(Gate {
        id: "g5-cleanup".to_string(),
        target: "cleanup max <= 2000 ms, all ok".to_string(),
        observed: format!("max={:.0} n={} fail={}", st.max_ms, st.n, st.fail),
        pass: st.fail == 0 && st.n > 0 && st.max_ms <= 2000.0,
    });
}

pub(crate) fn score_wall(
    walls: &[(String, f64)],
    prefix: &str,
    target: &str,
    budget: f64,
    gates: &mut Vec<Gate>,
) {
    let mut values: Vec<f64> = walls
        .iter()
        .filter(|(k, _)| k.starts_with(prefix) && !k.ends_with(" exit-ok"))
        .map(|(_, v)| *v)
        .collect();
    let single = values.len() == 1;
    values.sort_by(f64::total_cmp);
    let p95 = if values.is_empty() {
        0.0
    } else {
        values[(95_usize.saturating_mul(values.len()).div_ceil(100)).saturating_sub(1)]
    };
    gates.push(Gate {
        id: prefix.to_string(),
        target: target.to_string(),
        observed: if single {
            format!("wall={p95:.0} ms")
        } else {
            format!("p95={p95:.1} n={}", values.len())
        },
        pass: !values.is_empty() && p95 <= budget,
    });
}

pub(crate) fn score_builds(walls: &[(String, f64)], gates: &mut Vec<Gate>) {
    for id in ["g7-core-edit", "g7-render-edit", "g7-view-edit"] {
        let wall = walls
            .iter()
            .find(|(k, _)| k == &format!("{id} Ms"))
            .map_or(0.0, |(_, v)| *v);
        let ok = walls
            .iter()
            .find(|(k, _)| k == &format!("{id} exit-ok"))
            .map_or(0.0, |(_, v)| *v)
            > 0.5;
        gates.push(Gate {
            id: id.to_string(),
            target: "edit→verdict <= 2000 ms on a green verdict".to_string(),
            observed: format!("wall={wall:.0} ms exit_ok={ok}"),
            pass: ok && wall > 0.0 && wall <= 2000.0,
        });
    }
}

pub(crate) fn score_nextest_full(walls: &[(String, f64)], gates: &mut Vec<Gate>) {
    let wall = walls
        .iter()
        .find(|(k, _)| k == "g8-nextest-full Ms")
        .map_or(0.0, |(_, v)| *v);
    let ok = walls
        .iter()
        .find(|(k, _)| k == "g8-nextest-full exit-ok")
        .map_or(0.0, |(_, v)| *v)
        > 0.5;
    gates.push(Gate {
        id: "g8-nextest-full".to_string(),
        target: "full nextest wall <= 120 s on a green run".to_string(),
        observed: format!("wall={wall:.0} ms exit_ok={ok}"),
        pass: ok && wall > 0.0 && wall <= 120_000.0,
    });
}
