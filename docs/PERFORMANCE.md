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

### F12 before/after (all fixes: rows 13-18)

Same machine class as row 12 (Apple M5 Max, 18 CPUs, 128 GiB,
macOS 27, rustc 1.98.1): "before" is the F11/F12 base tree,
"after" adds the bounded op channel + `Feed` coalescing, the
thread-local shared renderers, and cheap `Session::meta`. Method:
a scratch release-mode bench (`/tmp`, path deps on the workspace,
not committed) with a counting global allocator and
`getrusage(RUSAGE_SELF)` peak RSS. Corpus: capture/control on a
quiet 80x24 session (300/200 samples); close over 25
spawn(printf-ready)+close cycles; shot over 12 `render_sample`
PNGs of an 80x24 mixed-style screen; flood = `seq 1 200000`
through an 80x24 PTY to natural exit + `wait_stable` drain.
Queue limits (Watcher caps, op-channel bound), explicit
`CancelToken`s, and decoded-pixel exact verification (PNG bytes
identical before/after: 895,917 B) throughout.

| # | Suite | Metric | Value |
|---|---|---|---|
| 13 | bench capture (before -> after) | observe_now mean / p95 | 0.152 / 0.227 ms -> 0.081-0.131 / 0.111-0.174 ms (run-to-run noise on a shared box; no mechanism changed: same round trip, same ~240 KiB allocs per read) |
| 14 | bench control (before -> after) | send_text round-trip mean / p95 | 0.149 / 0.224 ms -> 0.061-0.068 / 0.103-0.108 ms (same noise caveat; unchanged mechanism) |
| 15 | bench close (before -> after) | spawn+close mean / p95 | 56.2 / 62.8 ms -> 56.6 / 60.4 ms (unchanged, as expected: teardown path untouched) |
| 16 | bench shot (before -> after) | render_sample throughput; allocs | 72.8 png/s; 395 MB -> 104.8 png/s (1.44x); 228 MB (shared default renderer: faces parsed once per thread) |
| 17 | bench flood (before -> after) | exit wall; post-exit drain; revisions; peak RSS; allocs | 0.98 s; 3.01 s; 31,031 revs; 28.7 MB; 5.7 GB / 93M -> 0.27 s; 0.22 s (14x); ~4,550 revs (7x); 25.9 MB; 0.59 GB / 8.9M (10x). Peak RSS is dominated by the emulator's intended 10k-line scrollback either way; the queue win shows in drain latency, revision count, and allocator churn |
| 18 | `cargo test -p tuiscotti --test render_qual` (before -> after) | wall, same machine | 2.93 s (32 tests) -> 0.97 s (33 tests, one added): shared strict renderers, zero coverage change |

New cheap path (not a before/after: it did not exist): 20,000
`Session::meta()` reads complete in well under 1 s (<50 us each:
one short lock, no worker round trip, no screen clone), pinned by
`concurrent::session_meta_is_cheap_and_current`; the equivalent
`observe_now` loop would take ~seconds and allocate ~240 KiB per
read.
