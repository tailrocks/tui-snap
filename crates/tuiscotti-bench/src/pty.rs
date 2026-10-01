//! PTY scenarios: readiness, journey, cleanup, sweep (fixed 80x24).

use crate::driver::{Args, base, fill, measure};
use crate::emit::{Sink, short_debug};
use std::time::{Duration, Instant};
use tuiscotti::assert::render_sample;
use tuiscotti::diff::compare_png;
use tuiscotti::observe::screen_text;
use tuiscotti::tui::{CancelToken, Tui, process_exists};

const SIZE: &str = "80x24";

///
/// # Errors
///
/// Returns an error when spawn or the readiness wait fails.
fn ready_session() -> anyhow::Result<(tuiscotti::tui::Session, tuiscotti::Observation, Duration)> {
    let cancel = CancelToken::new();
    let spawn_start = Instant::now();
    let session = Tui::new([
        "/bin/sh",
        "-c",
        "printf 'menu: alpha\\nmenu: beta\\n'; sleep 30",
    ])
    .size(80, 24)
    .spawn()
    .map_err(|e| anyhow::anyhow!("spawn: {}", short_debug(&e)))?;
    let deadline = Instant::now() + Duration::from_secs(10);
    let obs = session
        .wait_predicate(
            |o| {
                let text = screen_text(&o.screen);
                text.contains("menu: alpha") && text.contains("menu: beta")
            },
            deadline,
            &cancel,
        )
        .map_err(|e| anyhow::anyhow!("readiness: {}", short_debug(&e)))?;
    Ok((session, obs, spawn_start.elapsed()))
}

/// One readiness→verdict sample: fresh observe + text check + render both +
/// exact compare. Full spawn→verdict travels in the detail. Sweep workers
/// tag `scenario` as `"sweep"` so scaling load never pools into the g4a
/// latency gate (mirrors the views suite).
#[must_use]
pub fn readiness_sample(scenario: &'static str, worker: u32, iter: u32) -> crate::emit::Sample {
    let mut s = base("pty", scenario, SIZE, "pty-fixture");
    s.case = "predicate";
    s.worker = worker;
    s.iter = iter;
    let (session, ready, spawn_wait) = match ready_session() {
        Ok(v) => v,
        Err(e) => {
            s.detail = format!("setup: {e:?}");
            return s;
        }
    };
    let (meas, ok, detail) = measure(|| {
        let fresh = match session.observe_now() {
            Ok(o) => o,
            Err(e) => return (false, format!("observe: {}", short_debug(&e))),
        };
        if !screen_text(&fresh.screen).contains("menu: beta") {
            return (false, "verify text missing".to_string());
        }
        let expected = match render_sample(&ready.screen) {
            Ok(x) => x,
            Err(e) => return (false, format!("sample: {}", short_debug(&e))),
        };
        let actual = match render_sample(&fresh.screen) {
            Ok(x) => x,
            Err(e) => return (false, format!("sample: {}", short_debug(&e))),
        };
        match compare_png(&expected.png, &actual.png) {
            Ok(v) => (
                v.pixels_equal,
                format!(
                    "spawn_wait_ns={} rev={}",
                    spawn_wait.as_nanos(),
                    fresh.revision
                ),
            ),
            Err(e) => (false, format!("compare: {e}")),
        }
    });
    fill(&mut s, &meas, ok, detail);
    if session.close().is_err() {
        s.ok = false;
        s.detail.push_str(" close_failed");
    }
    s
}

/// Readiness scenario: `samples` single-threaded readiness samples.
///
/// # Errors
///
/// Returns an error when a scenario step or JSONL write fails.
pub fn run_readiness(args: &Args, sink: &mut Sink) -> anyhow::Result<()> {
    for iter in 0..args.samples {
        let s = readiness_sample("readiness", 0, iter);
        sink.write(&s)?;
    }
    Ok(())
}

/// Journey: 3-transition `sh` drive (echo state1/2/3 + waits) then
/// exit + reap + close, end to end.
///
/// # Errors
///
/// Returns an error when a scenario step or JSONL write fails.
pub fn run_journey(args: &Args, sink: &mut Sink) -> anyhow::Result<()> {
    for iter in 0..args.samples.div_ceil(3).max(2) {
        let (meas, ok, detail) = measure(|| {
            let cancel = CancelToken::new();
            let session = match Tui::new(["/bin/sh"]).size(80, 24).spawn() {
                Ok(s) => s,
                Err(e) => return (false, format!("spawn: {}", short_debug(&e))),
            };
            // Output-gated transitions: each marker appears only in the
            // command's stdout, never in the typed line (which the PTY
            // echoes immediately), so every wait observes a real
            // input→execute→output round trip.
            for (command, marker) in [
                ("echo \"o1:$((40 + 2))\"\n", "o1:42"),
                ("echo \"o2:$((20 + 3))\"\n", "o2:23"),
                ("echo \"o3:$((9 * 9))\"\n", "o3:81"),
            ] {
                if session.send_text(command).is_err() {
                    return (false, format!("send {marker}"));
                }
                let deadline = Instant::now() + Duration::from_secs(10);
                if session
                    .wait_predicate(
                        |o| screen_text(&o.screen).contains(marker),
                        deadline,
                        &cancel,
                    )
                    .is_err()
                {
                    return (false, format!("wait {marker}"));
                }
            }
            if session.send_text("exit\n").is_err() {
                return (false, "send exit".to_string());
            }
            let deadline = Instant::now() + Duration::from_secs(10);
            if session.wait_exit(deadline, &cancel).is_err() {
                return (false, "wait_exit".to_string());
            }
            if session.close().is_err() {
                return (false, "close".to_string());
            }
            (true, "transitions=3 cleanup=close".to_string())
        });
        let mut s = base("pty", "journey", SIZE, "pty-fixture");
        s.case = "three-transitions";
        s.iter = iter;
        fill(&mut s, &meas, ok, detail);
        sink.write(&s)?;
    }
    Ok(())
}

fn pid_gone(session: &tuiscotti::tui::Session) -> bool {
    session.pid().is_none_or(|pid| !process_exists(pid))
}

/// Cleanup: close-idle / close-paste / flood-drain / cancel-close, each
/// sample gated at 2 s with an owned-process reap check.
///
/// # Errors
///
/// Returns an error when a scenario step or JSONL write fails.
pub fn run_cleanup(args: &Args, sink: &mut Sink) -> anyhow::Result<()> {
    let limit = Duration::from_secs(2).as_nanos();
    for iter in 0..args.samples.div_ceil(3).max(2) {
        let session = Tui::new(["/bin/sh", "-c", "sleep 30"])
            .size(80, 24)
            .spawn()
            .map_err(|e| anyhow::anyhow!("spawn: {}", short_debug(&e)))?;
        let meas = measure_close(&session);
        let mut s = base("pty", "cleanup", SIZE, "pty-fixture");
        s.case = "close-idle";
        s.iter = iter;
        let ok = meas.elapsed_ns <= limit && pid_gone(&session);
        fill(&mut s, &meas, ok, format!("reaped={}", pid_gone(&session)));
        sink.write(&s)?;
    }
    for iter in 0..args.samples.div_ceil(6).max(1) {
        let session = Tui::new(["/bin/sh", "-c", "sleep 30"])
            .size(80, 24)
            .spawn()
            .map_err(|e| anyhow::anyhow!("spawn: {}", short_debug(&e)))?;
        let send_ok = session.paste(&"x".repeat(1 << 20)).is_ok();
        let meas = measure_close(&session);
        let mut s = base("pty", "cleanup", SIZE, "pty-fixture");
        s.case = "close-paste";
        s.iter = iter;
        let ok = send_ok && meas.elapsed_ns <= limit && pid_gone(&session);
        fill(&mut s, &meas, ok, format!("send_ok={send_ok}"));
        sink.write(&s)?;
    }
    for iter in 0..args.samples.div_ceil(10).max(1) {
        flood_sample(sink, iter, limit)?;
    }
    for iter in 0..args.samples.div_ceil(6).max(1) {
        let cancel = CancelToken::new();
        cancel.cancel();
        let session = Tui::new(["/bin/sh", "-c", "sleep 30"])
            .size(80, 24)
            .spawn()
            .map_err(|e| anyhow::anyhow!("spawn: {}", short_debug(&e)))?;
        let deadline = Instant::now() + Duration::from_secs(10);
        let cancelled = session
            .wait_predicate(|_| false, deadline, &cancel)
            .is_err();
        let meas = measure_close(&session);
        let mut s = base("pty", "cleanup", SIZE, "pty-fixture");
        s.case = "cancel-close";
        s.iter = iter;
        let ok = cancelled && meas.elapsed_ns <= limit && pid_gone(&session);
        fill(&mut s, &meas, ok, format!("cancelled={cancelled}"));
        sink.write(&s)?;
    }
    Ok(())
}

fn measure_close(session: &tuiscotti::tui::Session) -> crate::driver::Meas {
    let (meas, _, _) = measure(|| {
        if session.close().is_ok() {
            (true, String::new())
        } else {
            (false, "close".to_string())
        }
    });
    meas
}

///
/// # Errors
///
/// Returns an error when a scenario step or JSONL write fails.
fn flood_sample(sink: &mut Sink, iter: u32, limit: u128) -> anyhow::Result<()> {
    let cancel = CancelToken::new();
    let session = Tui::new(["/bin/sh", "-c", "seq 1 200000"])
        .size(80, 24)
        .spawn()
        .map_err(|e| anyhow::anyhow!("spawn: {}", short_debug(&e)))?;
    let exit_start = Instant::now();
    let exit_ok = session
        .wait_exit(Instant::now() + Duration::from_secs(60), &cancel)
        .is_ok();
    let exit_wall = exit_start.elapsed().as_nanos();
    let (meas, ok, _) = measure(|| {
        let drain_ok = session
            .wait_stable(Instant::now() + Duration::from_secs(10), &cancel)
            .is_ok();
        let close_ok = session.close().is_ok();
        (drain_ok && close_ok, String::new())
    });
    let mut s = base("pty", "cleanup", SIZE, "pty-fixture");
    s.case = "flood-drain";
    s.iter = iter;
    let within = meas.elapsed_ns <= limit;
    fill(
        &mut s,
        &meas,
        exit_ok && ok && within,
        format!("exit_wall_ns={exit_wall}"),
    );
    sink.write(&s)?;
    Ok(())
}

/// PTY sweep: W threads × readiness samples. Prints `WALL_NS`.
///
/// # Errors
///
/// Returns an error when a worker or JSONL write fails.
pub fn run_sweep(args: &Args, sink: &mut Sink) -> anyhow::Result<()> {
    let workers = usize::try_from(args.workers.max(1)).unwrap_or(1);
    let total = usize::try_from(args.samples).unwrap_or(8).max(workers);
    let per = total.div_ceil(workers);
    let wall = Instant::now();
    std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(workers);
        for worker in 0..workers {
            handles.push(scope.spawn(move || {
                let mut out = Vec::new();
                for iter in 0..per {
                    if worker * per + iter >= total {
                        break;
                    }
                    out.push(readiness_sample(
                        "sweep",
                        u32::try_from(worker).unwrap_or(u32::MAX),
                        u32::try_from(iter).unwrap_or(u32::MAX),
                    ));
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
