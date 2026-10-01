# Tests, fixtures, and CI

## Layout

| Location | Contents |
|---|---|
| `crates/tuiscotti/tests/` | 20 suites: `cargo_bin_unified`, `cells`, `examples_lane`, `export`, `facade`, `g6_facade`, `grouped`, `import_compat`, `insta_spike`, `locate`, `p0_mutations`, `ratatui_views`, `render_qual`, `runner`, `screen`, `semant`, `snapshot`, `snapshot_lifecycle`, `snapshot_safety`, `tool_qualification` (269 tests) |
| `crates/tuiscotti-cli/tests/` | `agent_if`, `cli`, `epipe`, `observe`, `piped`, `readme_lock`, `vertical_slice_cli_error` (85 tests) |
| `crates/tuiscotti-runtime/tests/` | `tui`, `tui_shell` (PTY; need feature `pty`) |
| `crates/tuiscotti-render/`, `-insta/`, `xtask` | unit tests in `src/` (12 + 21 + 17 tests) |
| `crates/tuiscotti-fixtures/` | `fixture_app` model/view + `tests/{format_contracts, interaction_contracts, journey, render, underline, vertical_slice, view_contracts, visual}` + committed approvals (86 tests) |
| `crates/tuiscotti/examples/` | `01-pure-view` … `08-agent-workflow` (the learning lane) |

683 tests via `cargo nextest list --locked --offline
--all-features` on main `d996feb`; full run green 683/683 + 1 skipped
(2026-10-01; see [PERFORMANCE.md](PERFORMANCE.md)).
Doctest lane is separate (`cargo test --doc`).

## The fixture app

`tuiscotti-fixtures::fixture_app` is one model/view that doubles as
the PTY target and the headless-test source. Three fixture binaries
(`crates/tuiscotti-fixtures/Cargo.toml` `[[bin]]`):
`menu_fixture`, `streams_fixture`, `protocol_fixture`
(`tests/fixtures/apps/*.rs`). Committed approvals:

- `crates/tuiscotti-fixtures/tests/visual/approved/` — 24
  `*.frame.json` + `*.png` pairs (classic store).
- `crates/tuiscotti-fixtures/tests/snapshots/` — Insta snapshots
  (`.snap` + `.snap.png`) for journey/vertical-slice gates.
- `crates/tuiscotti-fixtures/tests/fixtures/` — consumer, data,
  expected, and slice fixtures.
- `crates/tuiscotti-fixtures/tests/SHA256SUMS` — pins every
  approval byte; re-record with `cargo xtask fixtures
  --bless-manifest` after a qualified change (a test fails
  otherwise).

## Examples lane (learning path)

Eight small programs, each one idea, each exiting 0 with an
`EXAMPLE-NN-OK` marker. Nothing writes outside temp dirs.

| # | Example | Idea |
|---|---|---|
| 01 | `01-pure-view` | Production draw closure → `Screen` → `assert_snapshot!` (pre-approved) |
| 02 | `02-styled-shot` | `assert_screenshot!`: canonical + PNG as one sample, evidence on disk |
| 03 | `03-piped-cli` | `Command` error cases: spawn errors vs exit codes, split streams |
| 04 | `04-interactive-tui` | PTY journey: spawn → `wait_predicate` → snapshot → close |
| 05 | `05-locators-waits` | `Locator::text` + `expect_visible` to one deadline |
| 06 | `06-artifacts-review` | `emit_four` + frozen root: pin artifacts, reject acceptance |
| 07 | `07-advanced-profiles` | Strict `RenderProfile`, `Strict` vs `Placeholder` missing policy |
| 08 | `08-agent-workflow` | `proto::execute` + `machine` JSON envelopes, no PTY |

```sh
cargo run -p tuiscotti --example 01-pure-view   # any single step
cargo test -p tuiscotti --test examples_lane    # runs all 8, asserts exit 0 + markers
```

## Running the suite

```sh
cargo nextest run --locked --offline --all-features   # preferred: parallel, ~33 s warm locally (32.7 s measured 2026-10-01)
cargo test --locked --offline                         # serial fallback
cargo test -p tuiscotti --no-default-features         # pure-view only, no PTY engine
cargo test --locked --offline --doc                   # doctests
```

Timings are local Apple-silicon numbers; see
[PERFORMANCE.md](PERFORMANCE.md). The 120 s CI fast-lane budget must
be re-proven with CI timings, not local ones.

## CI wiring

Gates run everywhere; approval happens only on a human workstation.
There is no flag or variable that approves — the only way to break
this rule is to run `accept` in CI. Don't.

```yaml
- name: Visual gates
  run: cargo nextest run --locked --offline --all-features

- name: Publish visual evidence (on failure too)
  if: always()
  uses: actions/upload-artifact@v4
  with:
    name: visual-evidence
    path: |
      crates/tuiscotti-fixtures/tests/visual/actual/
      crates/tuiscotti-fixtures/tests/visual/diff/
      crates/tuiscotti-fixtures/tests/visual/report.html
```

Notes:

- Determinism: same frame + same profile + same vendored font
  bytes = byte-identical PNGs (tested). Runners need no system fonts.
- Review flow for a red run: download `visual-evidence`, open
  `report.html`, reproduce locally if needed, then
  `tuiscotti accept --store <dir> <name>` locally and push the updated
  `approved/` tree.
- Parallel jobs are safe: per-name files + atomic renames.
  Concurrent jobs sharing one store directory only race on
  `report.html` (best-effort index); gate data never clobbers.
- Do not cache `actual/`, `diff/`, or `report.html` between runs —
  they are per-run evidence.

## Link and doc checks

Every relative link in `README.md`, `CONTRIBUTING.md`, and
`docs/*.md` must resolve to a file in the tree. Check:

```sh
cargo run -p xtask -- docs
```

`readme_lock.rs` additionally mirrors every README fence and pins
the CLI surface against `--help` — run it after any doc edit that
touches claimed behavior:
`cargo test -p tuiscotti-cli --test readme_lock`.
