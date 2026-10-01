# Performance

One current performance report for Tuiscotti: methodology, the committed
reproducible benchmark suite, budget scoreboards, and this-host numbers.
It replaces all prior scratch-harness notes (the F12 `/tmp` harness and the
`0f14262`/`75ff479` rows are retired; nothing below depends on them).

Status: **pre-merge final.** g1–g8 PASS on the green reruns
(`68b4967`/`36bbe64`, 2026-10-01; g8: 48 s wall, 659/659 + 1 skip).
Post-merge main `d996feb` runs 683/683 + 1 skipped (`cargo nextest`,
2026-10-01); the `cargo xtask bench` budget suite has not yet been
re-executed on main — re-run it before quoting g1–g7 numbers as
post-merge acceptance. The red-run caveats below are historical record.

E2 compliance: the harness is zero-`unsafe` first-party code under the
intact workspace lints (`unsafe_code` deny, no exceptions). A prior
revision measured per-op allocation via a counting global allocator and
peak RSS via `getrusage`; that unsafe instrumentation was removed in full
— per-op allocator counters no longer exist, and RSS is sampled from
Linux `VmHWM` only. Timing methodology (`Instant` around the documented
operation) is unchanged, so the wall-time tables below stay comparable
across the rework; the retired allocator rows do not, and are marked as
such. Nothing below is final until the clean-head rerun.

## How to run

```sh
cargo xtask bench                      # full suite, ~5 min on the reference host
cargo xtask bench --quick              # reduced samples, same suites
cargo xtask bench --suite views        # views|pty|cli|builds|nextest, or all
cargo xtask bench --out /tmp/bench-out # results dir (default benches/results/)
```

Exit 0 when every measured budget passes, 1 otherwise. `xtask` stays
zero-dependency: stats (p50/p95/tail/throughput) and JSON are hand-rolled.

## Suite layout

- `crates/tuiscotti-bench/` — measurement harness (publish=false,
  zero-`unsafe`). Two binaries emit raw per-sample JSONL; shared `rss`
  (Linux `VmHWM` peak-RSS sampling, std-only), `emit` (JSONL writer),
  `fixtures`
  (fixed 80x24/120x40/200x60 screens × plain/dense/unicode/scroll/overlay/
  cursor/resize journeys through the real production view functions),
  `driver` (`Instant` timing + CLI), and scenario modules.
  - `bench_views` — `canonical` (fresh capture + canonicalize + string
    compare), `full` (fresh capture + fresh `render_sample` + exact
    decoded-pixel `compare_png`; no content cache is consulted, every
    sample records `fresh_candidate=true cache_hits=0`), `compare`
    (equal/changed/corrupt/missing via `compare_png`), `cached`
    (fresh-renderer / warm-shared / empty-cache / populated / no-cache),
    `sweep` (W-thread medium-screen full verify + `WALL_NS` throughput).
  - `bench_pty` — `readiness` (deterministic 80x24 printf fixture:
    explicit-predicate readiness, then fresh observe + text check + render
    both + exact compare; reports readiness→verdict and spawn→verdict),
    `journey` (output-gated 3-transition `sh` drive + exit + reap +
    close), `cleanup` (close-idle / 1 MiB close-paste / `seq 1 200000`
    flood-drain / cancel-close, each gated at 2 s with a reap check),
    `sweep` (W concurrent readiness sessions; tagged `sweep` so scaling
    load never pools into the g4a latency gate).
- `crates/xtask/src/bench*.rs` — orchestrator: builds release bins, runs
  scenarios and probes, parses JSONL, scores gates, writes the envelope.
- `benches/results/<stamp>-*.jsonl` — raw per-sample JSONL (one object per
  line: suite/scenario/size/journey/case/cache/worker/iter/elapsed_ns/
  rss/rss_units/ok/detail).
- `benches/results/<stamp>-envelope.json` — repro envelope: command,
  corpus SHA-256 (bench sources + fixture views + `Cargo.lock`), source
  SHA + dirty-file count, toolchain, profile, hardware, cache states,
  concurrency, limits, per-group stats, walls, notes, and the budget
  scoreboard. `<stamp>-nextest-full.log` keeps the full-suite transcript.

## Methodology

- Timers: `Instant` (monotonic) around the documented operation only;
  harness overhead (JSONL writes, RSS file reads) stays outside the
  timed region. Percentiles use the nearest-rank method over `ok` samples;
  failures are counted, never silently dropped, and any `fail > 0` fails
  the gate.
- Cache states: canonical/full never consult the content cache; the
  `cached` matrix isolates renderer reuse (fresh construct vs thread-local
  shared) from cache behavior (empty miss+store vs populated steady-state
  hit with full PNG-decode validation vs `no_cache` instance, `stores=0`
  asserted). Per-cache `CacheOptions` select the mode; no env globals.
- Concurrency: views/PTY sweeps at 1/2/4/8/16 workers plus a bounded
  32-worker views oversubscription case; nextest sweeps via `--test-threads`
  on the CPU-bound `render_qual` suite and the PTY-bound `tui` suite.
- Memory: sampled process peak RSS only. On Linux the harness reads
  `VmHWM` from `/proc/self/status` (kibibytes) with plain file I/O after
  the clock stops; on every other OS there is no portable std-only
  peak-RSS source, so samples honestly record `rss=0` with
  `rss_units="unknown"` instead of guessing. Per-op allocator counters
  were removed with the unsafe counting allocator (E2) and are not
  measured. Aggregates report maxima; independent process peaks are
  never summed into a fake "total peak".
- Budgets g7/g8 additionally require a green verdict (`exit_ok`): a red
  run's wall time is recorded but cannot pass, since skipped/failed targets
  distort it. g3 discards nothing: one recorded warm-up absorbs first-exec
  OS costs (notably macOS first-run verification, ~700 ms observed) and is
  kept as `cli-warmup`; the gate scores the 25 warm fresh-process samples.
- Test-runner children run WITHOUT the xtask recursion guard (they may
  legitimately invoke `xtask` helpers; no test invokes `bench`, so no
  cycle can form). Runs 1–2 predate this fix: their g8 verdicts are void.
- Noise policy: a single run never establishes a regression. The report
  keeps every run's raw data; gates are per-run, and cross-run variance is
  quantified below. Reference numbers come from the primary run; older and
  contended runs stay on record as labeled history.

## Reference host

All numbers below were measured on this host (labeled H1):

- Apple M5 Max (`Mac17,6`), 18 CPUs, 128 GiB RAM (`hw.memsize`
  137438953472), page size 16384, macOS 27.0, APFS
  (`/dev/disk3s5`, ~2.0 TiB free of ~3.6 TiB at measure time).
- Toolchain: `rustc 1.98.1 (48a229cea 2026-09-01)`,
  `cargo 1.98.1 (797e8a9bc 2026-08-05)`,
  `cargo-nextest 0.9.143 (60fa45f63 2026-08-04)`, `--offline` throughout,
  warm `~/.cargo` registry. Micro-bench profile: release; builds/nextest:
  normal dev/test profiles.

## Budget scoreboard (primary)

Primary: run 3 (`d56d39a0`, dirty=31) for views/cli/builds/nextest,
run 4 (`cdecbbe0`, dirty=31) for pty. History: run 1 (`ce1ea590`,
quiet-ish) and run 2 (`5cfdd79`, heavily contended) follow the table.

| # | Budget | Primary observed | Verdict |
|---|--------|------------------|---------|
| g1 | canonical p95 ≤ 5/10/25 ms (80x24/120x40/200x60) | 0.52 / 0.86 / 2.01 (n=280/size, fail=0) | PASS, 5–12x headroom |
| g2 | full p95 ≤ 50/100/250 ms, fresh render, 0 cache hits | 10.1 / 15.2 / 27.9 (n=70/size, fail=0) | PASS, 5–9x headroom |
| g3 | fresh release CLI p95 ≤ 250 ms | 9.2, n=25 (warmup 688) | PASS, ~27x headroom |
| g4a | PTY readiness→verdict p95 ≤ 100 ms | 18.6, p50 15.5, n=30, fail=0 (run 4) | PASS, 5x headroom |
| g4b | 3-transition journey p95 ≤ 1 s | 78, p50 67, n=10, fail=0 (run 4, output-gated) | PASS, ~13x headroom |
| g5 | cleanup max ≤ 2 s, all ok | 228, n=23, fail=0 (run 4) | PASS, ~9x headroom |
| g6 | 4-worker medium full ≥ 2.5x single | 3.38x (wall1=899 ms, wall4=266 ms) | PASS |
| g7 | edit→verdict ≤ 2 s (core/render/view) | 0.74 / 0.39 / 0.56–1.41 s, green | PASS (profile fix `fc04640`; study §) |
| g8 | full nextest wall ≤ 120 s, green | 48 s wall, 659/659 pass, 1 skip | PASS (green rerun `36bbe64`, 2026-10-01) |

History (same budgets, older harness revisions — see footnotes):

| # | Run 1 (`ce1ea590`, quiet-ish) | Run 2 (`5cfdd79`, contended) |
|---|--------------------------------|------------------------------|
| g1 | 0.55 / 1.03 / 2.43 — PASS | 1.54 / 6.79 / 27.10 — FAIL (200x60 tail) |
| g2 | 10.7 / 16.7 / 38.4 — PASS | 81.2 / 87.4 / 205.1 — FAIL (80x24 tail) |
| g3 | 6.5 (warmup 986) — PASS | 5.3 (warmup 709) — PASS |
| g4a | 72.7, n=94 pooled¹ — PASS | 71.8, n=94 pooled¹ — PASS |
| g4b | 68, p50 37, echo-gated² — PASS | 69, p50 7, echo-gated² — PASS |
| g5 | 225 — PASS | 226 — PASS |
| g6 | 3.72x — PASS | 3.48x — PASS |
| g7 | 1899 / 857 / 4868 — FAIL (view) | 1158 / 727 / 2796 — FAIL (view) |
| g8 | 89.5 s, red — VOID³ | 34.3 s, red — VOID³ |

¹ Runs 1–3 pooled sweep samples into `readiness`; run 4 tags sweeps
separately (the run-3 pooled p95 was 138 ms, driven by 8-worker tails up
to 198 ms — a scaling finding, not fixture latency). ² Pre-output-gating
fixture: markers could match PTY echo; run 3+ gates on stdout-only
markers. ³ Runs 1–2 inherited `TUISCOTTI_XTASK_ACTIVE=1` into nextest,
failing xtask's own CLI tests; fixed by scrubbing the guard for
test-runner children (verified: those tests pass standalone 4/4).

## This-host numbers (primary runs)

Distributions are tight when the box is quiet (max ≈ p95 ≈ p50 for
canonical/full); run-2 tails (canonical-200x60 max 85 ms, compare max
1025 ms, dozens of samples beyond 2×p50) are contention spikes, not
algorithmic cliffs. Per-group detail lives in the envelope `groups`
arrays; spokesman rows (run 3 unless noted):

| Scenario/size | p50 | p95 | max | Note |
|---|---|---|---|---|
| canonical 80x24 / 120x40 / 200x60 | 0.31 / 0.76 / 1.88 | 0.52 / 0.86 / 2.01 | 0.55 / 0.87 / 2.10 | capture+canonical+compare |
| full 80x24 / 120x40 / 200x60 | 9.7 / 14.9 / 27.6 | 10.1 / 15.2 / 27.9 | 10.3 / 15.5 / 28.0 | fresh PNG render dominates |
| compare equal 80x24 / 120x40 / 200x60 | 1.59 / 3.24 / 7.25 | 1.61 / 3.28 / 7.41 | — | decode + memcmp |
| compare changed 80x24 / 120x40 / 200x60 | 17.0 / 39.1 / 93.4 | 17.3 / 39.8 / 94.6 | — | hybrid diff + diff-PNG encode |
| compare corrupt / missing 200x60 | 11.0 / 7.2 | 11.1 / 7.3 | — | decode attempt, then fail |
| PTY readiness→verdict, single (run 4) | 15.5 | 18.6 | — | spawn→verdict ~8–10 ms + verify |
| PTY sweep pooled (run 4, 1/2/4/8 workers) | 18.4 | 122.4 | 185.2 | 8-worker tail: contention cost |
| PTY journey, output-gated (run 4) | 67 | 78 | — | 3 real transitions + cleanup |
| cleanup worst case (flood-drain) | 84 | 207 | 227 | all reaped, all ≤ 2 s |

Cache matrix, per-op p50 ms (n=20/state, run 3; `populated` = pure
steady-state hit: key derivation + file hit with full PNG-decode
validation + byte equality):

| Size | fresh-renderer | warm-shared | empty-cache | populated | no-cache |
|---|---|---|---|---|---|
| 80x24 | 5.5 | 1.8 | 7.7 | 1.5 | 1.9, stores=0 |
| 120x40 | 7.6 | 3.9 | 11.1 | 3.1 | 4.0, stores=0 |
| 200x60 | 12.6 | 8.9 | 21.1 | 6.8 | 9.2, stores=0 |

A populated hit beats a warm render by only ~20–25%: key derivation +
file hit + full-decode validation costs ~75% of a render at these sizes.
An optimization lead, not a verdict defect. The no-cache `stores=0`
invariant held in every sample of all runs.

Scaling curves (equal work per point; wall ms):

| Workers | 1 | 2 | 4 | 8 | 16 | 32 |
|---|---|---|---|---|---|---|
| views sweep, 120x40 full ×64 (run 3) | 899 | 489 | 266 | 178 | 133 | 126 |
| pty sweep, readiness ×16 (run 4) | 1440 | 1149 | 1005 | 926 | — | — |
| nextest render_qual (run 3) | 9082 | 2757 | 1463 | 940 | 811 | — |
| nextest tui (run 3) | 174 | — | 196 | — | — | — |

Views scale ~7x at 32 workers on 18 cores (render-bound, independent
sessions); PTY readiness barely scales past 2 workers (spawn-bound,
1.55x at 8 workers); `render_qual` scales 11x at `-j16`; the tiny `tui`
suite is PTY-latency-bound and flat (174 → 196 ms is noise).

Memory (prior-methodology history, not current evidence: sampled with
the retired `getrusage` instrument on macOS, bytes; the current harness
samples Linux `VmHWM` only and macOS samples record `rss=0/unknown`.
Re-measure on the Linux qualifier before quoting):

| Scope | Metric | Value |
|---|---|---|
| views process | max sampled peak RSS | 875,937,792 (~835 MiB run 3; 702 MiB run 1) |
| pty process | max sampled peak RSS | 234,127,360 (~223 MiB run 4; 82 MiB run 1) |

Retired with the unsafe counting allocator (E2, no longer measured):
per-op thread allocator counters (canonical-200x60 ~5.3 MiB mean,
full-200x60 ~133 MiB mean, compare-changed-200x60 ~622 MiB max,
readiness-80x24 ~30 MiB mean).

No per-worker RSS isolation is claimed: RSS is process-wide and sampled.
PTY peak RSS varies run to run (82 → 223 MiB): the peak depends on
allocator timing under concurrent sweep sessions plus flood scrollback;
both values are single sampled maxima from honest runs, reported as a
range.

Builds (dev profile, warm registry):

| Measurement | Run 3 (primary) | History |
|---|---|---|
| warm no-op `cargo build --workspace` ×3 | 130 / 116 / 126 ms | 127 / 75 / 78 ms (run 1) |
| scratch-target clean build (temp `CARGO_TARGET_DIR`, warm registry) | 21.1 s | 13.8 s (run 1) |
| edit→verdict, core touch → `tuiscotti-render --lib` (green) | 1622 ms | 1899 / 1158 ms |
| edit→verdict, renderer touch → same (green) | 1018 ms | 857 / 727 ms |
| edit→verdict, view touch → `tuiscotti-fixtures --test view_contracts` (green) | 5701 ms — misses 2 s | 4868 / 2796 ms — miss reproduces 3/3 |
| full nextest (658 tests) | 61.9 s wall, 654 pass, 4 fail, 1 skip | 89.5 s (646, run 1) / 34.3 s (646, run 2) |

### g7-view-edit optimization study (HEAD `a24bba7` + profile fix)

Probe-equivalent `cargo test -p tuiscotti-fixtures --test view_contracts`
after a one-file `views/menu.rs` touch, `CARGO_BUILD_JOBS=4`, sequential, on
host H1. Touches alternated two contents (real-edit semantics; strictly
harder than the probe's content-identical rewrite, which additionally
benefits from content-keyed cache reuse).

| Condition | Run walls (s) | Verdict |
|---|---|---|
| HEAD, contended (load 20–50, sibling nextest) | 4.01 / 2.19 / 3.00 / 2.62 | FAIL — reproduces the run-1–3 miss |
| HEAD, quiet (load ~5) | 1.51 / 1.35 / 1.30 | PASS — HEAD already passes quiet |
| + `[profile.test] debug=line-tables-only`, quiet | 1.41 / 1.19 / 1.27 then 1.33 / 0.90 / 0.86 / 0.56 / 0.74 | PASS, green 12/12 every run |

Dominant cost (`--timings` + `-Z time-passes`): one view touch rebuilds 5
test-profile units — fixtures lib + `view_contracts` + the 3 PTY fixture
binaries (coupled via `CARGO_BIN_EXE`; `cargo build --test` builds them too,
so no probe-command flag can shed them). Quiet per-unit walls pre-fix: test
0.7 s, bins 0.6–0.7 s, lib 0.6 s; post-fix: 0.4 / 0.4 / 0.2 s. Full-DWARF
codegen for all five was pure verdict-gate overhead; line tables keep failure
backtraces file:line-accurate, and dev/release profiles keep full debuginfo.
Rejected: `--no-default-features` probe (2.25–3.56 s, no significant move —
the cost is unit count + DWARF, not the pty closure), splitting
`view_contracts` into its own package (needs `.github/ci` inventory regen +
docs churn outside this fix's scope). Steady-state floor (nothing dirty):
~0.6–0.8 s wall. Core/render legs post-fix (quiet, probe-method touch):
0.74 s / 0.39 s, green — no regression.

## Validity and caveats

- Dirty tree, shared box: 31+ dirty files during every run; sibling
  agents compiled and tested concurrently. Run 2 quantifies the cost:
  canonical-200x60 p95 2.43 → 27.10 ms, full-80x24 p95 10.7 → 81.2 ms,
  with p50s drifting up to 2.6x. Primary-run numbers are the reference;
  contended numbers bound the noise.
- Red suite, resolved by green rerun: the primary-head red cause (4
  `tuiscotti-runtime` PTY-test failures: `env_remove_drops_one_var`,
  `env_clear_starts_empty`, `invalid_cwd_fails_spawn`,
  `close_input_eofs_raw_cat` — the sibling runtime/spawn refactor area,
  F07) is gone at `36bbe64`. The first three now pass; the fourth was
  split by the lifecycle batch (6c5b559) into
  `close_input_eofs_canonical_cat` + `close_input_raw_cat_is_data_not_eof`
  (both pass). Green rerun 2026-10-01: 48 s wall (build+run; 16.7 s
  test-time), 659/659 pass (1 leaky), 1 skip, on a quiet tree (load ~3
  after a 3-min wait for a sibling velnor-new nextest job; load 4.6 at
  end). Prior red walls (34–90 s) stay as history in the scoreboard.
- g7-view-edit reproduces 3/3 on contended runs (4.9 s → 2.8 s → 5.7 s,
  all green verdicts): incremental `view_contracts` after a one-file view
  touch misses the 2 s budget. Not a harness artifact (same touch replays
  the developer flow; content unchanged; verdict green each time) — but a
  quiet-box study at HEAD `a24bba7` passes (1.51 / 1.35 / 1.30 s), so the
  miss is contention-driven; the `[profile.test]
  debug=line-tables-only` fix (`fc04640`) closed it (0.56–1.41 s, 8 runs,
  green 12/12 each) — see the optimization study above. Prior miss walls
  stay as history.
- Run-3 g4a pooled p95 (138 ms) is superseded by run 4 (18.6 ms): the
  earlier figure mixed 8-worker sweep contention into the latency gate.
  The sweep tail itself (max 185 ms at 8 workers) is reported above as a
  scaling finding, not hidden.
- Prior-history claims retired: F12 `/tmp`-harness rows and the
  `0f14262`/`75ff479` table described other trees and other harnesses and
  are not comparable to this suite. This file is now the only performance
  report; raw history lives in `benches/results/`.

## Findings for follow-up

1. g7-view-edit — RESOLVED (study above): 5 test-profile units per touch
   (lib + test + 3 `CARGO_BIN_EXE`-coupled fixture binaries), full-DWARF
   codegen on all five. Fix: `[profile.test] debug="line-tables-only"`.
   Contended-box misses were environmental (quiet HEAD already passes);
   re-prove on the quiet final-head rerun like every other number here.
2. compare-changed cost (93 ms p50 at 200x60): the hybrid-diagnostic +
   diff-PNG path dominates mismatch verdicts; stream or downscale the
   diagnostic (never the strict verdict).
3. Cache-hit value is thin (~20–25% under a warm render): full-decode
   validation dominates; a cheaper integrity check would buy the hit path
   back (without weakening rejection of corrupt entries).
4. PTY concurrency barely scales (1.55x at 8 workers; 185 ms tail):
   spawn-bound. Expected, but it caps PTY-heavy suite parallelism.
5. g8 green re-proven at `36bbe64` (2026-10-01, quiet tree, 48 s wall)
   — DONE locally. Still open: re-prove the 120 s budget on the qualified
   CI runner (local walls do not transfer).
