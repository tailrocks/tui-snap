# tui-snap redesign: requirement ledger (82 items)

Status key: `done` = implemented + qualifying test green; `red` = failing regression test committed, fix in flight;
`open` = gap confirmed in current code; `partial` = some behavior exists, needs rework/qualification;
`missing` = no implementation; `keep` = current behavior is the target, needs qualification lock-in.
A checkbox without a qualifying test is not completion. Evidence = test names + commands.

Frozen discovery: 2026-09-28. Revisions reviewed: tui-snap `9dc86da`, tui-test `7afb14b` (=HEAD),
terminal-control `c1d4f95` (=HEAD, anomalyco), insta `064742e` (=HEAD).
Competitor deltas: tui-test writes missing snapshot even when update=false (do not copy);
terminal-control extractor resolves RGB / swaps inverse / skips continuations / collapses underlines (presentation, not source contract).

## P0 — Verification correctness (M0)

| ID | Criterion | Status | Implementation | Tests | Verified |
|----|-----------|--------|----------------|-------|----------|
| C01 | Same cells+dims, different decoded pixels fail strict check | red | `src/diff.rs` remove `ansi_matched` bypass | `tests/p0_mutations.rs::c01_*` (ignored) | `cargo test --test p0_mutations -- --ignored c01_` must pass after fix |
| C02 | Actual images rendered from candidate, never copied from approved | open | `src/grouped.rs` actual-evidence path | needs test | no |
| C03 | Exact decoded RGBA/opaque comparison; recompression passes | red | `src/diff.rs` + `AlphaPolicy` | `tests/p0_mutations.rs::c03_*` (ignored) | same pattern |
| C04 | Perceptual score never establishes strict equality; invalid tolerances rejected | red | `src/diff.rs` separate diagnostic | `tests/p0_mutations.rs::c04_*` (ignored) | same pattern |
| C05 | One verdict engine for test/CLI/report | red | `src/grouped.rs` report via persisted verdict | `tests/p0_mutations.rs::c05_*` (ignored) | same pattern |
| C06 | Missing approved PNG fails; no in-memory regen | red | `src/snapshot.rs` remove regen path | `tests/p0_mutations.rs::c06_*` (ignored) | same pattern |
| C07 | Outcomes `#[must_use]`; mismatch fails | red | `src/snapshot.rs` outcome type | `tests/p0_mutations.rs::c07_*` (ignored) | same pattern |
| C08 | Completion manifest; interrupted/partial never matched | red | `src/snapshot.rs` + `src/grouped.rs` manifest | `tests/p0_mutations.rs::c08_*` (ignored) | same pattern |
| C09 | Original approval hashes unchanged | done | `docs/APPROVAL-BASELINE.md` (35 files + fonts) | re-hash command in doc | `adfda5e`, baseline committed; re-verify at M7 |
| C10 | README matches actual behavior | open | `README.md` | doc review vs tests | no |

## P1 — Model, API, direct views (M1)

| ID | Criterion | Status | Implementation | Tests | Verified |
|----|-----------|--------|----------------|-------|----------|
| M01 | Validated immutable screen/observation; one capture = one revision | partial | `src/frame.rs` Frame v3 → `Screen`/`Observation` | `tests/cells.rs` (partial) | no |
| M02 | All baseline source distinctions preserved | partial | `src/frame.rs` colors/mods/continuations/cursor | `tests/tool_qualification.rs` (partial) | no |
| M03 | Extended state assertable without bloating snapshots | missing | underline style/color, links, modes, title, bells, clipboard, palette, graphics | none | no |
| M04 | 1-row/1-col static fixtures work (no PTY limits) | open | dimension validation split | needs test | no |
| M05 | Direct Ratatui closure/buffer/TestBackend adapters, no round trip | partial | `src/ratatui.rs` | `tests/render.rs` (partial) | no |
| M06 | Stateful views + explicit cursor without Widget bound | partial | `src/ratatui.rs::draw_frame` | needs test | no |
| M07 | Region/mask policies; no silent wide-grapheme split | missing | new region API | none (`src/ratatui.rs:121` silently clamps today) | no |
| M08 | Stable identity separate from runtime metadata | partial | `Provenance` split | `tests/cells.rs` (partial) | no |
| M09 | Pure-view consumer builds without pty/native deps | partial | `Cargo.toml` default/`pty` features | `tests/fixtures/consumer` | no |

## P2 — Insta lifecycle (M2)

All missing. Baseline: `snapshot::Store` + `grouped::GroupedStore` (custom, to be superseded by native Insta).

| ID | Criterion | Status | Implementation | Tests | Verified |
|----|-----------|--------|----------------|-------|----------|
| I01 | `assert_snapshot!` styled canonical state via native Insta | missing | facade macro + projection | none | no |
| I02 | `assert_screenshot!` one sample: evidence before failure | missing | facade macro + generation binding | none | no |
| I03 | Insta custom decoded-pixel Comparator (public API) | missing | comparator | none | no |
| I04 | Compound lifecycle: partial acceptance can't mix generations | missing | needs integration spike first | none | no |
| I05 | No vendored Insta internals; upstream hook if needed | missing | — | none | no |
| I06 | Evolving (review) vs frozen (read-only, no self-heal) policies | missing | policy types | none | no |
| I07 | 4-artifact export from one generation + read-only frozen importer | missing | exporter + importer | none | no |

## P2 — Nextest (M2)

| ID | Criterion | Status | Implementation | Tests | Verified |
|----|-----------|--------|----------------|-------|----------|
| N01 | Plain `#[test]`, no custom harness | keep | already true; keep | suite itself | no |
| N02 | Stable baseline identity + isolated attempt identity | missing | test-context + nextest adapter | none | no |
| N03 | Prefer nextest-remapped executable paths | missing | resolver | none | no |
| N04 | Relocated archive support | missing | resolver + path policy | none | no |
| N05 | JUnit correlation without rewriting status | missing | reporter | none | no |
| N06 | Required-scenario manifest; filtered = visibly partial | missing | manifest + gate | none | no |
| N07 | No completion marker = incomplete; journal survives | missing | journal | none | no |
| N08 | Failed-attempt artifacts survive retry/stress | missing | attempt isolation | none | no |
| N09 | Parallel-safe: no shared env/session/ports/paths | missing | isolation | none | no |
| N10 | Doc examples executed (tests or explicit doctest lane) | missing | harness | none | no |

## P3 — CLI + PTY runtime (M3)

| ID | Criterion | Status | Implementation | Tests | Verified |
|----|-----------|--------|----------------|-------|----------|
| R01 | Piped `Command`: separate stdio bytes, exit/signal/timeout | missing | new runtime API | none | no |
| R02 | Deadlock-safe drain; limits/spooling; non-UTF-8 | missing | new runtime API | none | no |
| R03 | Isolated envs (HOME/XDG/cwd, child-only env) | partial | `src/pty.rs:180` area | `tests/pty.rs` (partial) | no |
| R04 | Upstream PTY/emulator crates; NO vendored termlens | open | REMOVE `vendor/termlens`; qualify upstream | n/a (deletion) | no |
| R05 | Ghostty binding qualified first; no silent substitution | missing | backend qualification record | none | no |
| R06 | Atomic revisioned observations | partial | `src/pty.rs:96` area | `tests/pty.rs` (partial) | no |
| R07 | Waits don't block cancel/observe/other sessions | missing | scheduler rework | none | no |
| R08 | Owned cleanup on return/error/panic/cancel; no double-panic Drop | partial | `src/pty.rs:171` area | `tests/pty.rs` (partial) | no |
| R09 | Parent-hard-kill guardian/containment; escaping descendants explicit | missing | guardian | none | no |
| R10 | Separate readiness/stability/frame/exit waits | partial | `src/pty.rs:217` area | `tests/pty.rs` (partial) | no |
| R11 | Full input surface (chords/raw/paste/mouse/focus/resize/signal) | partial | `src/pty.rs` input | `tests/pty.rs` (partial) | no |
| R12 | Explicit shell sessions; shell-cmd exit ≠ child exit | missing | shell integration | none | no |
| R13 | Terminal-state assertions (palette/clipboard/bells/title/modes/scrollback/links) | missing | assertions | none | no |
| R14 | Bounded raw replay invariant to chunk boundaries | partial | `src/ansi.rs:31` | `tests/tool_qualification.rs` (partial) | no |

## P4 — Queries/assertions (M4): all missing (only Q10 primitives exist)

| ID | Criterion | Status | Tests | Verified |
|----|-----------|--------|-------|----------|
| Q01 | Fresh text/regex/style/link/region locators on current revision | missing | none | no |
| Q02 | Composition/scope (within/before/after/nth/and/or/filter) | missing | none | no |
| Q03 | Strict actions; scrollback not clickable | missing | none | no |
| Q04 | Retryable observational assertions, one deadline | missing | none | no |
| Q05 | Actions execute once (no repeat of destructive ops) | missing | none | no |
| Q06 | Optional semantic provider (never inferred from appearance) | missing | none | no |
| Q07 | Semantics select real input, never call controllers | missing | none | no |
| Q08 | not_present_now / eventually_absent / remains_absent | missing | none | no |
| Q09 | Deterministic event/clock harness (caller hooks) | missing | none | no |
| Q10 | Region/palette/cursor invariants | partial (prims) | `tests/cells.rs` partial | no |

## P5 — Rendering/evidence (M5)

| ID | Criterion | Status | Implementation | Tests | Verified |
|----|-----------|--------|----------------|-------|----------|
| V01 | One renderer for direct/live/imported | partial | `src/render.rs` | `tests/render.rs`, `tests/visual.rs` | no |
| V02 | Explicit hashed font packs; frozen | keep | `src/profile.rs` + `assets/fonts` | baseline doc | hash-locked; qualify in M5 |
| V03 | Grapheme qualification corpus | partial | renderer | `tests/render.rs` partial | no |
| V04 | Source widths control layout | keep | grid-first layout | `tests/render.rs` partial | no |
| V05 | Missing glyph fails strict approval by default | partial | fidelity sidecar exists | `tests/render.rs` partial | no |
| V06 | Source vs display vs safe export; concealment ≠ redaction | partial | exporter review needed | partial | no |
| V07 | Blink intent in state; declared phase in stills | partial | cursor/model | partial | no |
| V08 | Content-addressed caches; corrupt rejected; no-cache mode | partial | renderer cache | partial | no |
| V09 | Portable offline reports | partial | report builder | `tests/visual.rs` partial | no |
| V10 | Opt-in byte contracts for ANSI/TXT/HTML | partial | grouped gates | `tests/grouped.rs` partial | no |

## P6 — Advanced (M6)

| ID | Criterion | Status | Implementation | Tests | Verified |
|----|-----------|--------|----------------|-------|----------|
| A01 | One typed op/result/error protocol (Rust+CLI+machine) | missing | — | none | no |
| A02 | Optional named sessions | missing | — | none | no |
| A03 | Live human observe/interact, no multiplexer required | missing | — | none | no |
| A04 | Trace journal + offline viewer | missing | — | none | no |
| A05 | Replay-vs-rerun distinction | missing | — | none | no |
| A06 | Bounded recordings; screenshot/cast/GIF/APNG; MP4 via external encoder | missing | — | none | no |
| A07 | Graphics payload inspection; compositing only when qualified | missing | — | none | no |
| A08 | Agent CLI + schema/capabilities + optional MCP | missing | — | none | no |
| A09 | Thin JS/TS + Python clients, no second engine | missing | — | none | no |
| A10 | Read-only importers (tui-snap stores + selected competitor traces) | partial | `tools/migrate` exists | partial | no |
| A11 | Performance qualification (cold/warm/latency/memory/suite) | missing | — | none | no |
| A12 | macOS/Linux conformance; truthful Windows ConPTY subset | missing | — | none | no |

## Milestone gates (M0–M7)

- M0: all C red tests green + C02/C10 tests added and green; unlawful states unrepresentable where cheap.
- M1: `Screen`/`Observation`/policy types; ratatui adapters without ANSI round trip; M09 consumer check.
- M2: native Insta review + nextest matrix (normal/filter/shard/retry/stress/remap/cancel) on vertical slice.
- M3: piped `Command` + owned PTY + qualified backend + R04 vendor removal.
- M4: locators/assertions/semantics/event helpers per Q.
- M5: pinned rendering + full evidence + offline review per V.
- M6: A01–A12 per rows above.
- M7: old runtime/API removed; read-only import only; C09 re-verified; docs = implemented contracts.

First vertical slice (early M2): pure settings view + piped CLI error + PTY settings-navigation journey,
all under nextest with native Insta expectations, failure evidence, verified cleanup.
