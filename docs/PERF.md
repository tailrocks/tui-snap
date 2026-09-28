# Performance qualification (A11)

Local-only measurements. Not CI results, not a fast-lane proof.

## Method

- Date (UTC): 2026-09-28. Branch: `redesign/rust-first-testing-platform`.
- Hardware: Apple M5 Max, 18 CPUs (`hw.ncpu`), 128 GiB RAM
  (`hw.memsize` 137438953472), macOS 27.0 (26A428).
- Toolchain: `rustc 1.98.1 (48a229cea 2026-09-01)`,
  `cargo 1.98.1`, `cargo-nextest 0.9.143`, `rust-toolchain.toml`
  pins `channel = "1.98.1"`.
- Registry cache state: warm (`~/.cargo` populated; all timed runs used
  `--locked --offline` except where noted).
- Compiler cache: `cargo` on PATH resolves to a mise shim fronting
  mbx 1.18.0 (shared object cache, always on). "True cold" runs below
  bypassed it via the direct toolchain cargo
  (`~/.rustup/toolchains/1.98.1-aarch64-apple-darwin/bin/cargo`).
- Wall times measured with `date +%s` around the command (1 s resolution,
  except the millisecond rows which used `date +%s%N`).
- Caveat: other agents edited this tree concurrently. Two new test files
  (`tests/import_compat.rs`, `tests/observe.rs`) appeared between the
  first and last measurement, so suite totals below are "as observed",
  not a fixed corpus. Re-run on a quiet tree before quoting.

## Results

| # | Measurement | Wall | Self-reported test/compile time |
|---|-------------|------|---------------------------------|
| 1 | Cold build, clean target dir, mbx bypassed, warm registry (`cargo build`) | 11 s | cargo: 11.59 s |
| 2 | Cold target dir, warm mbx cache (`cargo build`) | 7 s | cargo: 6.35 s (257 mbx hits) |
| 3 | Warm no-op build, mbx-managed target (`cargo build`) | 4 s | cargo: 3.15 s |
| 4 | Full `cargo test --locked --offline`, exit 0 | 129 s | per-suite sum; slowest single test-time: `visual` 17.73 s |
| 5 | Full `cargo nextest run --locked --offline`: 379/379 pass | 43 s | nextest summary: 43.13 s |
| 6 | `cargo test --test tui`: 25 pass | 2 s | test-time 1.12 s → ~45 ms/test avg |
| 7 | Single PTY test, binary-direct (`tui-*/chord_press_sends_key --exact`) | 63 ms | test-time 0.06 s |
| 8 | Single CLI piped capture (`tuisnap capture --out /tmp/perf-cap -- echo hello`) | 10 ms | child `Exit(0)`; manifest + stdout/stderr artifacts written |
| 9 | `cargo test --test render_qual`: 21 pass (render throughput) | 17 s | test-time 15.93 s → ~760 ms/test avg (font rasterization heavy) |
| 10 | `cargo test --test render`: 29 pass | 13 s | test-time 11.09 s |

Suite size at measure time: 379 tests per `cargo nextest list`
(~371 `#[test]` in `tests/*.rs` + lib + 5 doc-tests; moving target,
see caveat).

## Artifact sizes

| Artifact | Size |
|----------|------|
| `tests/snapshots` (committed insta snapshots + PNGs) | 388 K |
| `target/debug/tuisnap` (debug CLI) | 69 M |
| Full debug target dir, true-cold build (`/tmp/perf-truecold/debug`) | 1.4 G |
| nextest archive (`tests.tar.zst`, 28 binaries + std) | 270 M |

## Fast-lane assessment vs the 120 s target

- Full `cargo test`: **129 s — misses** the 120 s CI fast-lane target
  on this machine by ~9 s. The `cargo test` runner executes test
  binaries serially; render/visual suites dominate.
- Full `cargo nextest run`: **43 s — passes** with 77 s of headroom.
  Same 379 tests, parallel across binaries.
- Verdict: the fast lane must run nextest (optionally sharded —
  see `docs/CONFORMANCE.md`, partitions split 186/193), not
  `cargo test`. These are local Apple-silicon numbers; CI runs
  `linux-x64` GitHub-hosted runners (see `.github/ci/project.toml`),
  so the 120 s budget must be re-proven with CI timings, not this file.
