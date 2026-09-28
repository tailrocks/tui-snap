# Conformance (A12) + nextest matrix (N) + consumer proof (M09)

## 1. Platform matrix (A12)

Date (UTC): 2026-09-28. Branch: `redesign/rust-first-testing-platform`.
Machine/toolchain: see `docs/PERF.md` (Apple M5 Max, macOS 27.0,
rustc 1.98.1, nextest 0.9.143).

| Platform | Status | Evidence |
|----------|--------|----------|
| macOS (aarch64) | TESTED here | Full `cargo test` green (129 s wall); full `cargo nextest run` 379/379 green (43 s). Details in §3. |
| Linux (x86_64) | CI-ONLY, unexecuted locally | No local Linux run in this session. CI lanes (`ci-pr.yml`, `ci-main.yml`, `ci-unit-rust.yml`, `nightly.yml`) all run `ubuntu-24.04`, unit `platform = "linux-x64"` (`.github/ci/project.toml`). Linux green must come from CI, not this file. |
| Windows (ConPTY) | NOT RUN anywhere; compiles only | `cargo check --target x86_64-pc-windows-gnu --tests`: **0 errors** (lib + all test targets). `portable-pty 0.9.0` ships a ConPTY backend (`NativePtySystem = win::conpty::ConPtySystem`, Win10 1809+), but no Windows test process has ever executed here: no local run, and **no Windows CI lane exists** (every workflow is `runs-on: ubuntu-24.04`). Windows is never green until a CI lane runs it. |

### Windows compile-check notes (honest deltas)

- Required a networked `cargo fetch --target x86_64-pc-windows-gnu`
  first: Windows-only deps (`miow 0.6.1`, …) are absent from the
  default fetch closure, so a bare `--offline` Windows check fails at
  download, not at code.
- 5 warnings, all dead-code in the non-unix stubs (expected shape):
  `ChildIds` fields `pgid`/`sid`/`start` never read;
  `MAX_PS_LINES`, `MAX_SURVIVORS`, `MAX_SWEEP_TARGETS`, `SWEEP_SETTLE`
  never used.

### cfg-gated platform exclusions (supported-subset ledger)

Unix-only code that degrades or compiles out on Windows
(line numbers as observed 2026-09-28; tree is moving):

- `src/tui.rs:826-829` — `.exe` suffix handling; compiles on both.
- `src/tui.rs:361` — `Signal::number` (`libc::SIG*`) is `#[cfg(unix)]`.
- `src/tui_shell.rs:1300,1523,1530` — unix guardian/process-group
  machinery is `#[cfg(unix)]`.
- `src/tui_shell.rs:1539-1546` — `#[cfg(not(unix))]` stubs:
  `ChildIds::capture` returns `None`, group sweep reports
  `Containment::Unsupported` (explicit, not silent).
- `src/command.rs:571-576` — `#[cfg(not(unix))] classify`: no signal
  reporting outside unix; maps to `Termination::Exit`, never invents
  a signal.
- `src/proto.rs:1103,1205,1237` vs `1215,1250` — unix / non-unix
  session paths.
- `tests/piped.rs:73` — `signal_death_is_distinct_from_exit` is
  `#[cfg(unix)]` (needs `/bin/sh` + SIGTERM).
- `tests/cli.rs:403,628` — `#[cfg(unix)]` blocks: owner-only
  (`0o700`) runtime-dir mode assertion; `0o755` chmod on a trap
  script. Skipped, not asserted, on Windows.

### What CI must run (for a truthful Windows claim)

1. Add a `windows-latest` lane running the same unit commands
   (`fmt --check`, `clippy -D warnings`, `nextest run` or
   `cargo test`) on this branch's manifest.
2. Until then, every Windows statement stays at "compiles for
   `x86_64-pc-windows-gnu`; ConPTY path unverified".

## 2. Pure-view consumer proof (M09)

`tests/fixtures/consumer` was broken against the new API: it referenced
`tuisnap::ansi::replay_raw`, `tuisnap::pty::Session`, and
`tuisnap::termlens::Screen`, none of which exist (no `ansi`/`pty`/
`termlens` modules; PTY surface is `tuisnap::tui` behind feature `pty`).
Fixed minimally to a true pure-view consumer:

- `Cargo.toml`: `tuisnap = { path = "../../..", default-features = false }`
  plus `ratatui 0.30` (same spec as the workspace dev-dependency) to
  drive the pure-view adapter.
- `src/main.rs`: `Frame::blank` + styled `Cell` round-trip,
  `ratatui::widget_frame(Paragraph)` → `Frame`, and a
  `tuisnap::Screen` type mention. No spawn, no PTY, no emulator.

Verification (this machine):

- `cargo build --offline --locked` in the fixture after one `cargo fetch`:
  exit 0; binary runs and prints the proof line.
- `cargo tree -e normal | grep -iE "portable-pty|alacritty_terminal"`:
  no matches — the emulator crates are absent. (`libc` still appears
  transitively via crossterm/mio; that is the ubiquitous unix shim,
  not the PTY emulator.)
- Root `cargo check --no-default-features --locked`: exit 0.

## 3. Nextest matrix (N03/N04/N09-class evidence)

Runner: `cargo-nextest 0.9.143`. No `.config/nextest.toml` in repo
(default profile). All runs `--locked --offline` on the macOS machine
above; 379 tests listed at measure time.

| # | Run | Result |
|---|-----|--------|
| 1 | Normal: `cargo nextest run` | 379 run, 379 pass, 0 skipped, 43.13 s |
| 2 | Filtered: `-E 'binary(cells)'` | 9 run, 9 pass |
| 3 | Sharded: `--partition hash:1/2` | 186 run, 186 pass (1 leaky), 193 skipped, 31.6 s |
| 4 | Sharded: `--partition hash:2/2` | 193 run, 193 pass (4 leaky), 186 skipped, 17.5 s |
| 5 | Retry flag: `-E 'binary(cells)' --retries 1` | 9 run, 9 pass; flag accepted, 0 retries needed — the retry-after-failure path was NOT exercised (no failing test available) |
| 6 | Stress: `-E 'binary(vertical_slice)' --stress-count 2` | 2/2 iterations pass, 3.17 s |
| 7 | Archive: `cargo nextest archive --archive-file /tmp/nx-arch/tests.tar.zst` | 28 binaries + std, 270 M, archived in 0.23 s (note: `p0_mutations` test target emitted 4 build warnings during archive) |
| 8 | Relocated run: `cargo nextest run --archive-file … --extract-to /tmp/nx-extract -E 'binary(cells) + binary(vertical_slice)'`, cwd `/tmp` (outside workspace) | 11 run, 11 pass (2 leaky) — N04 relocated-remap path works (requires pre-created `--extract-to` dir; nextest does not mkdir it) |
| 9 | Cancel: SIGINT to the `cargo nextest` parent 4 s into the ~16 s `render_qual` suite | Did NOT stop the run: all 21 tests completed and passed (signal observed by the cargo shim, exit -2 on the parent). Single-SIGINT cancel is not prompt through this wrapper; no claim made about direct nextest cancel. |

Shard flag note: this nextest has no `--shard-count`/`--shard-index`
(`error: unexpected argument '--shard-count'`); the supported spelling
is `--partition hash:<i>/<n>`. Rows 3–4 are that spelling.

Leak-detection note: nextest flagged a few "leaky" tests
(1 + 4 across the two shards; `cells::json_round_trip_…` and
`cells::corrupt_imports_…` in the archive run) while still passing
them. The flagged set varies run to run; recorded as an observation,
not a verdict — leak triage belongs to the owning suites, not this file.

## 4. Open gaps

- Linux: no local execution; covered only by the ubuntu-24.04 CI lanes.
- Windows: compiles, never runs; no CI lane. ConPTY behavior,
  signal/exit-code mapping, and the `Containment::Unsupported`
  guardian path are all unverified at runtime.
- Retry-after-failure (row 5) and prompt-cancel (row 9) paths are
  documented as not-proven, not green.
- Leaky-test flags (§3 note) are untriaged.
- All timings are single samples from a concurrently-edited tree;
  re-run on a quiet tree for quotable numbers.
