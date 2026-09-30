//! Views scenarios: canonical-only, full screenshot, compare cases.

use crate::driver::{Args, base, fill, measure};
use crate::emit::Sink;
use crate::fixtures::{JOURNEYS, SIZES, render_journey, render_journey_changed};
use tuiscotti::assert::render_sample;
use tuiscotti::diff::compare_png;
use tuiscotti::screen::canonical_string;

/// Canonical-only: fresh capture + canonicalize + string compare.
///
/// # Errors
///
/// Returns an error when a scenario step or JSONL write fails.
pub fn run_canonical(args: &Args, sink: &mut Sink) -> anyhow::Result<()> {
    for &k in &args.sizes {
        let (cols, rows, label) = SIZES[k];
        for journey in JOURNEYS {
            let expected = canonical_string(
                &render_journey(journey, cols, rows)
                    .map_err(|e| anyhow::anyhow!("canonical setup {label} {journey}: {e:?}"))?,
            );
            for iter in 0..args.samples {
                let (meas, ok, detail) = measure(|| {
                    let screen = match render_journey(journey, cols, rows) {
                        Ok(s) => s,
                        Err(e) => return (false, format!("render: {e:?}")),
                    };
                    let canon = canonical_string(&screen);
                    let eq = canon == expected;
                    (eq, format!("bytes={} equal={eq}", canon.len()))
                });
                let mut s = base("views", "canonical", label, journey);
                s.cache = "capture-only";
                s.iter = iter;
                fill(&mut s, &meas, ok, detail);
                sink.write(&s)?;
            }
        }
    }
    Ok(())
}

/// Full screenshot: fresh capture + fresh render + exact decoded-pixel
/// compare. No content cache is consulted (`cache_hits=0` in every detail),
/// so a cache-hit-only run cannot satisfy this gate.
///
/// # Errors
///
/// Returns an error when a scenario step or JSONL write fails.
pub fn run_full(args: &Args, sink: &mut Sink) -> anyhow::Result<()> {
    for &k in &args.sizes {
        let (cols, rows, label) = SIZES[k];
        for journey in JOURNEYS {
            let screen0 =
                render_journey(journey, cols, rows).map_err(|e| anyhow::anyhow!("{e:?}"))?;
            let expected = render_sample(&screen0)
                .map_err(|e| anyhow::anyhow!("{e:?}"))?
                .png;
            for iter in 0..args.full {
                let (meas, ok, detail) = measure(|| {
                    let screen = match render_journey(journey, cols, rows) {
                        Ok(s) => s,
                        Err(e) => return (false, format!("render: {e:?}")),
                    };
                    let sample = match render_sample(&screen) {
                        Ok(s) => s,
                        Err(e) => return (false, format!("sample: {e:?}")),
                    };
                    match compare_png(&expected, &sample.png) {
                        Ok(v) => (
                            v.pixels_equal,
                            format!(
                                "pixels_equal={} fresh_candidate=true cache_hits=0 png={}",
                                v.pixels_equal,
                                sample.png.len()
                            ),
                        ),
                        Err(e) => (false, format!("compare: {e}")),
                    }
                });
                let mut s = base("views", "full", label, journey);
                s.cache = "warm-shared";
                s.iter = iter;
                fill(&mut s, &meas, ok, detail);
                sink.write(&s)?;
            }
        }
    }
    Ok(())
}

/// Compare cases: equal / changed / corrupt / missing via `compare_png`.
///
/// # Errors
///
/// Returns an error when a scenario step or JSONL write fails.
pub fn run_compare(args: &Args, sink: &mut Sink) -> anyhow::Result<()> {
    for &k in &args.sizes {
        let (cols, rows, label) = SIZES[k];
        let png = render_sample(
            &render_journey("plain", cols, rows).map_err(|e| anyhow::anyhow!("{e:?}"))?,
        )
        .map_err(|e| anyhow::anyhow!("{e:?}"))?
        .png;
        let changed = render_sample(
            &render_journey_changed("plain", cols, rows).map_err(|e| anyhow::anyhow!("{e:?}"))?,
        )
        .map_err(|e| anyhow::anyhow!("{e:?}"))?
        .png;
        let corrupt = png[..png.len() / 2].to_vec();
        for case in ["equal", "changed", "corrupt", "missing"] {
            for iter in 0..args.cmp {
                let (meas, ok, detail) = measure(|| match case {
                    "equal" => match compare_png(&png, &png) {
                        Ok(v) => (v.pixels_equal, format!("pixels_equal={}", v.pixels_equal)),
                        Err(e) => (false, format!("compare: {e}")),
                    },
                    "changed" => match compare_png(&png, &changed) {
                        Ok(v) => (!v.pixels_equal, format!("pixels_equal={}", v.pixels_equal)),
                        Err(e) => (false, format!("compare: {e}")),
                    },
                    "corrupt" => match compare_png(&png, &corrupt) {
                        Ok(v) => (false, format!("unexpected equal={}", v.pixels_equal)),
                        Err(_) => (true, "rejected corrupt input".to_string()),
                    },
                    _ => match compare_png(&png, &[]) {
                        Ok(v) => (false, format!("unexpected equal={}", v.pixels_equal)),
                        Err(_) => (true, "rejected missing input".to_string()),
                    },
                });
                let mut s = base("views", "compare", label, "plain");
                s.case = case;
                s.cache = "n/a";
                s.iter = iter;
                fill(&mut s, &meas, ok, detail);
                sink.write(&s)?;
            }
        }
    }
    Ok(())
}
