//! Views scenarios: render-cache matrix + worker sweep.

use crate::driver::{Args, base, fill, measure};
use crate::emit::Sink;
use crate::fixtures::{SIZES, render_journey, render_varied};
use std::time::Instant;
use tuiscotti::assert::render_sample;
use tuiscotti::diff::compare_png;
use tuiscotti::profile::{MissingGlyphPolicy, RenderProfile};
use tuiscotti::render::{CacheOptions, RenderCache, Renderer, render_screen_png};

fn strict_profile() -> RenderProfile<'static> {
    RenderProfile::vendored().with_missing(MissingGlyphPolicy::Placeholder)
}

/// Cache matrix: fresh-renderer / warm-shared / empty-cache / populated /
/// no-cache. Miss and hit paths use distinct screens per iteration.
///
/// # Errors
///
/// Returns an error when a scenario step or JSONL write fails.
pub fn run_cached(args: &Args, sink: &mut Sink) -> anyhow::Result<()> {
    for &k in &args.sizes {
        let (cols, rows, label) = SIZES[k];
        let rp = strict_profile();
        let screen = render_journey("plain", cols, rows).map_err(|e| anyhow::anyhow!("{e:?}"))?;
        render_states(args, sink, &rp, &screen, label)?;
        empty_cache(args, sink, &rp, cols, rows, label)?;
        populated_cache(args, sink, &rp, cols, rows, label)?;
        no_cache(args, sink, &rp, &screen, label)?;
    }
    Ok(())
}

fn render_states(
    args: &Args,
    sink: &mut Sink,
    rp: &RenderProfile<'_>,
    screen: &tuiscotti::Screen,
    label: &'static str,
) -> anyhow::Result<()> {
    for iter in 0..args.cache_n {
        let (meas, ok, detail) = measure(|| {
            let mut r = match Renderer::for_render_profile(rp) {
                Ok(r) => r,
                Err(e) => return (false, format!("build: {e:?}")),
            };
            match r.render_screen_png(screen) {
                Ok(png) => (true, format!("png={}", png.len())),
                Err(e) => (false, format!("render: {e:?}")),
            }
        });
        let mut s = base("views", "cached", label, "plain");
        s.cache = "fresh-renderer";
        s.iter = iter;
        fill(&mut s, &meas, ok, detail);
        sink.write(&s)?;
    }
    for iter in 0..args.cache_n {
        let (meas, ok, detail) =
            measure(
                || match Renderer::with_strict(rp, |r| r.render_screen_png(screen)) {
                    Ok(png) => (true, format!("png={}", png.len())),
                    Err(e) => (false, format!("render: {e:?}")),
                },
            );
        let mut s = base("views", "cached", label, "plain");
        s.cache = "warm-shared";
        s.iter = iter;
        fill(&mut s, &meas, ok, detail);
        sink.write(&s)?;
    }
    Ok(())
}

fn empty_cache(
    args: &Args,
    sink: &mut Sink,
    rp: &RenderProfile<'_>,
    cols: u16,
    rows: u16,
    label: &'static str,
) -> anyhow::Result<()> {
    let dir = tempfile::tempdir().map_err(|e| anyhow::anyhow!("{e:?}"))?;
    let mut cache = RenderCache::open(dir.path(), &[]).map_err(|e| anyhow::anyhow!("{e:?}"))?;
    for iter in 0..args.cache_n {
        let (meas, ok, detail) = measure(|| {
            let screen = match render_varied(cols, rows, iter) {
                Ok(s) => s,
                Err(e) => return (false, format!("render: {e:?}")),
            };
            let key = RenderCache::key_for(&screen, rp);
            if cache.get(&key).is_some() {
                return (false, "unexpected hit".to_string());
            }
            let png = match render_screen_png(&screen, rp) {
                Ok(p) => p,
                Err(e) => return (false, format!("render: {e:?}")),
            };
            match cache.put(&key, &png) {
                Ok(()) => (true, format!("miss stores={}", cache.stores())),
                Err(e) => (false, format!("put: {e:?}")),
            }
        });
        let mut s = base("views", "cached", label, "plain");
        s.cache = "empty-cache";
        s.iter = iter;
        fill(&mut s, &meas, ok, detail);
        sink.write(&s)?;
    }
    Ok(())
}

fn populated_cache(
    args: &Args,
    sink: &mut Sink,
    rp: &RenderProfile<'_>,
    cols: u16,
    rows: u16,
    label: &'static str,
) -> anyhow::Result<()> {
    let dir = tempfile::tempdir().map_err(|e| anyhow::anyhow!("{e:?}"))?;
    let mut cache = RenderCache::open(dir.path(), &[]).map_err(|e| anyhow::anyhow!("{e:?}"))?;
    // Steady-state hit path: one stored key, looked up repeatedly. Only the
    // key derivation + file hit (incl. full PNG-decode validation) is timed.
    let screen = render_varied(cols, rows, 0).map_err(|e| anyhow::anyhow!("{e:?}"))?;
    let key = RenderCache::key_for(&screen, rp);
    let png = render_screen_png(&screen, rp).map_err(|e| anyhow::anyhow!("{e:?}"))?;
    cache
        .put(&key, &png)
        .map_err(|e| anyhow::anyhow!("{e:?}"))?;
    for iter in 0..args.cache_n {
        let (meas, ok, detail) = measure(|| {
            let lookup = RenderCache::key_for(&screen, rp);
            match cache.get(&lookup) {
                Some(hit) => (
                    hit == png,
                    format!("hit hits={} png={}", cache.hits(), hit.len()),
                ),
                None => (false, "unexpected miss".to_string()),
            }
        });
        let mut s = base("views", "cached", label, "plain");
        s.cache = "populated";
        s.iter = iter;
        fill(&mut s, &meas, ok, detail);
        sink.write(&s)?;
    }
    Ok(())
}

fn no_cache(
    args: &Args,
    sink: &mut Sink,
    rp: &RenderProfile<'_>,
    screen: &tuiscotti::Screen,
    label: &'static str,
) -> anyhow::Result<()> {
    let dir = tempfile::tempdir().map_err(|e| anyhow::anyhow!("{e:?}"))?;
    let mut cache =
        RenderCache::open_with_options(dir.path(), &[], CacheOptions { no_cache: true })
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;
    for iter in 0..args.cache_n {
        let (meas, ok, detail) = measure(|| {
            let key = RenderCache::key_for(screen, rp);
            if cache.get(&key).is_some() {
                return (false, "no-cache hit".to_string());
            }
            let png = match render_screen_png(screen, rp) {
                Ok(p) => p,
                Err(e) => return (false, format!("render: {e:?}")),
            };
            match cache.put(&key, &png) {
                Ok(()) => (cache.stores() == 0, format!("stores={}", cache.stores())),
                Err(e) => (false, format!("put: {e:?}")),
            }
        });
        let mut s = base("views", "cached", label, "plain");
        s.cache = "no-cache";
        s.iter = iter;
        fill(&mut s, &meas, ok, detail);
        sink.write(&s)?;
    }
    Ok(())
}

/// Worker sweep: medium-screen full verify across W threads (expected PNG
/// shared). Prints `WALL_NS` for throughput scoring.
///
/// # Errors
///
/// Returns an error when a worker or JSONL write fails.
pub fn run_sweep(args: &Args, sink: &mut Sink) -> anyhow::Result<()> {
    let (cols, rows, label) = SIZES[1];
    let expected =
        render_sample(&render_journey("plain", cols, rows).map_err(|e| anyhow::anyhow!("{e:?}"))?)
            .map_err(|e| anyhow::anyhow!("{e:?}"))?
            .png;
    let workers = usize::try_from(args.workers.max(1)).unwrap_or(1);
    let total = usize::try_from(args.samples).unwrap_or(64).max(workers);
    let per = total.div_ceil(workers);
    let wall = Instant::now();
    std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(workers);
        for worker in 0..workers {
            let expected = &expected;
            handles.push(scope.spawn(move || {
                let mut out = Vec::new();
                for iter in 0..per {
                    if worker * per + iter >= total {
                        break;
                    }
                    let (meas, ok, detail) = measure(|| {
                        let screen = match render_journey("plain", cols, rows) {
                            Ok(s) => s,
                            Err(e) => return (false, format!("render: {e:?}")),
                        };
                        let sample = match render_sample(&screen) {
                            Ok(s) => s,
                            Err(e) => return (false, format!("sample: {e:?}")),
                        };
                        match compare_png(expected, &sample.png) {
                            Ok(v) => (v.pixels_equal, format!("pixels_equal={}", v.pixels_equal)),
                            Err(e) => (false, format!("compare: {e}")),
                        }
                    });
                    let mut s = base("views", "sweep", label, "plain");
                    s.cache = "warm-shared";
                    s.worker = u32::try_from(worker).unwrap_or(u32::MAX);
                    s.iter = u32::try_from(iter).unwrap_or(u32::MAX);
                    fill(&mut s, &meas, ok, detail);
                    out.push(s);
                }
                out
            }));
        }
        for handle in handles {
            match handle.join() {
                Ok(worker_samples) => {
                    for s in worker_samples {
                        sink.write(&s)?;
                    }
                }
                Err(_) => return Err(anyhow::anyhow!("sweep worker panicked")),
            }
        }
        Ok::<(), anyhow::Error>(())
    })?;
    println!(
        "WALL_NS={} WORKERS={workers} ITERS={total}",
        wall.elapsed().as_nanos()
    );
    Ok(())
}
