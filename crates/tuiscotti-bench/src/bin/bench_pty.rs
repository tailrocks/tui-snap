//! `bench_pty`: live-PTY fixture bench (JSONL, fixed 80x24).

use tuiscotti_bench::driver::{parse_args, wants};
use tuiscotti_bench::emit::Sink;
use tuiscotti_bench::pty;

const HELP: &str = "usage: bench_pty --out <jsonl> [--scenario all|readiness,journey,cleanup,sweep]\n       [--samples N] [--workers W] [--quick]";

///
/// # Errors
///
/// Returns an error when a scenario step or JSONL write fails.
fn run() -> anyhow::Result<()> {
    let cli: Vec<String> = std::env::args().collect();
    let args = parse_args(&cli, HELP, 30)?;
    let mut sink = Sink::create(&args.out)?;
    if wants(&args, "readiness") {
        pty::run_readiness(&args, &mut sink)?;
    }
    if wants(&args, "journey") {
        pty::run_journey(&args, &mut sink)?;
    }
    if wants(&args, "cleanup") {
        pty::run_cleanup(&args, &mut sink)?;
    }
    if wants(&args, "sweep") {
        pty::run_sweep(&args, &mut sink)?;
    }
    let count = sink.finish()?;
    println!("SAMPLES={count} OUT={}", args.out.display());
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("bench_pty: {e:?}");
        std::process::exit(1);
    }
}
