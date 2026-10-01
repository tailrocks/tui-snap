//! `bench_views`: in-process capture/render/compare micro-bench (JSONL).

use tuiscotti_bench::driver::{HelpText, parse_args, wants};
use tuiscotti_bench::emit::Sink;
use tuiscotti_bench::{views, views_cache};

const HELP: &str = "usage: bench_views --out <jsonl> [--scenario all|canonical,full,compare,cached,sweep]\n       [--size all|80x24,120x40,200x60] [--samples N] [--full N] [--cmp N]\n       [--cache-n N] [--workers W] [--quick]";

///
/// # Errors
///
/// Returns an error when a scenario step or JSONL write fails.
fn run() -> anyhow::Result<()> {
    let cli: Vec<String> = std::env::args().collect();
    let args = parse_args(&cli, HELP, 40)?;
    let mut sink = Sink::create(&args.out)?;
    if wants(&args, "canonical") {
        views::run_canonical(&args, &mut sink)?;
    }
    if wants(&args, "full") {
        views::run_full(&args, &mut sink)?;
    }
    if wants(&args, "compare") {
        views::run_compare(&args, &mut sink)?;
    }
    if wants(&args, "cached") {
        views_cache::run_cached(&args, &mut sink)?;
    }
    if wants(&args, "sweep") {
        views_cache::run_sweep(&args, &mut sink)?;
    }
    let count = sink.finish()?;
    println!("SAMPLES={count} OUT={}", args.out.display());
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        if let Some(help) = e.downcast_ref::<HelpText>() {
            println!("{help}");
            return;
        }
        eprintln!("bench_views: {e:?}");
        std::process::exit(1);
    }
}
