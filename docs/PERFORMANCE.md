# Performance

Local-only measurements. Not CI results, not a fast-lane proof.
Rows 1–10/M1–M4 were measured at head `0f14262`; row 11 re-measures
the full nextest run at head `75ff479` on the same machine class.
Unmeasured at the current head: cold/warm builds, serial `cargo
test`, per-suite splits, artifact sizes, peak RSS — quoted below
only as prior-head evidence, not current numbers.

## Method

- Date (UTC): 2026-09-28 (rows 1–10, M1–M4), re-verified
  2026-09-29 for method only. Branch:
  `redesign/rust-first-testing-platform`, head `0f14262` at rewrite.
- Hardware: Apple M5 Max, 18 CPUs, 128 GiB RAM, macOS 27.0.
- Toolchain: `rustc 1.98.1`, `cargo-nextest 0.9.143`,
  `rust-toolchain.toml` pins `channel = "1.98.1"`.
- Registry warm (`~/.cargo` populated); all runs `--locked
  --offline`. `cargo` on PATH is a cache shim (mbx); "true cold"
  rows bypassed it via the direct toolchain cargo.
- Wall times via `date +%s` (1 s resolution; ms rows via
  `date +%s%N`). Peak RSS via single-sample `/usr/bin/time -l`
  (debug builds).
- Caveat: other agents edited the tree concurrently during
  measurement; suite totals are "as observed". Re-run on a quiet
  tree before quoting.

## Results

| # | Measurement | Wall | Detail |
|---|-------------|------|--------|
| 1 | Cold build, clean target, shim bypassed, warm registry | 11 s | cargo: 11.59 s |
| 2 | Cold target, warm cache | 7 s | cargo: 6.35 s |
| 3 | Warm no-op build | 4 s | cargo: 3.15 s |
| 4 | Full `cargo test --locked --offline`, exit 0 | 129 s | slowest suite: `visual` 17.73 s test-time |
| 5 | Full `cargo nextest run --locked --offline --all-features` | 52 s | 415/415 pass (1 leaky) at measure time; current tree holds 413 `#[test]` (see TESTING.md) |
| 6 | `cargo test --test tui` (PTY suite) | 2 s | test-time 1.12 s → ~45 ms/test avg |
| 7 | Single PTY test (`tui-* chord_press_sends_key --exact`) | 63 ms | test-time 0.06 s |
| 8 | Single piped capture (`tuiscotti capture --out … -- echo hello`) | 10 ms | child `Exit(0)` + artifacts |
| 9 | `cargo test --test render_qual` (render throughput) | 17 s | test-time 15.93 s → ~760 ms/test avg (font rasterization heavy) |
| 10 | `cargo test --test render` | 13 s | test-time 11.09 s |
| 11 | Full `cargo nextest run --locked --offline --all-features` at `75ff479` | 21 s | 519/519 pass, 0 skipped; warm build cache |

## Artifact sizes

| Artifact | Size |
|----------|------|
| Committed insta snapshots + PNGs | 388 K |
| `target/debug/tuiscotti` (debug CLI) | 69 M |
| Full debug target dir (true-cold) | 1.4 G |
| nextest archive (31 binaries + std) | 310 M |

## Peak RSS (single samples, debug builds)

| # | Measurement | Peak RSS |
|---|-------------|----------|
| M1 | Pure-view snapshot test incl. PNG render/compare | ~333 MiB |
| M2 | Piped `tuiscotti capture -- echo hello` | ~6.2 MiB |
| M3 | One PTY session test | ~4.1 MiB |
| M4 | One 200×60 mixed-script render → 4048×2568 PNG (5.6 MiB) + sidecar | ~380 MiB |

M1/M4 peaks are font rasterization + PNG encode of debug builds,
not a release profile. Re-run before quoting.

## Fast-lane assessment vs the 120 s target

- Full `cargo test`: **129 s — misses** the 120 s CI fast-lane
  target on this machine by ~9 s (serial test binaries;
  render/visual suites dominate); not re-run at `75ff479`.
- Full `cargo nextest run`: **52 s at `0f14262`, 21 s at `75ff479`
  — passes** with wide headroom (row 11, warm cache).
- Verdict: the fast lane must run nextest, not `cargo test`. These
  are local Apple-silicon numbers; CI runs `linux-x64`
  GitHub-hosted runners, so the 120 s budget must be re-proven
  with CI timings, not this file.
