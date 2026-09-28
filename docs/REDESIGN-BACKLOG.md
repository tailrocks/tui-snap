# tui-snap redesign: implementation backlog and acceptance contract

Status: proposed architecture, not implemented or benchmarked.
Research date: 2026-09-28.

## Product decision

Keep tui-snap as the product. Build a Rust-first CLI/TUI testing toolkit with three complementary entry points: piped processes, real PTY applications, and detached production Ratatui views. Make Insta the default snapshot-review integration and cargo-nextest the default runner. Reuse upstream PTY/emulator crates; do not embed a competing full test framework or maintain modified third-party source copies.

The end-user installs one library facade (tuisnap) and, optionally, one CLI. Rust view tests require no daemon, child process, network, shell, or native terminal engine. Ordinary #[test] remains the default test registration mechanism. All API names below are proposed.

## Source baseline

| Project | Inspected revision |
|---|---|
| tailrocks/tui-snap | 9dc86daff1dcbf20805b145916e8f04e9515f929 |
| microsoft/tui-test | 7afb14b3c4075d24a7b9bf1a05175717f253821c |
| anomalyco/terminal-control | c1d4f95e4f1b7638f6229e9bfc9599a955f95ce2 |
| mitsuhiko/insta | 064742e9b7b2f3eaabb4724069e739ddf23d8227 |

Repository code, not prior comparisons, is authoritative. In particular, current tui-snap schema v3 already preserves hidden and blink flags. Do not file those as missing features. Its current comparison shortcuts, storage/report disagreement, and runtime/API gaps are separate issues. [S1–S8]

## Architecture and ownership

| Package boundary | Responsibility | Must not require |
|---|---|---|
| tuisnap-core | Validated screen/observation types, comparison policies, query evaluation, protocol capabilities | PTY, rasterizer, CLI, async runtime |
| tuisnap-render | Explicit-font renderer, image comparison, fidelity diagnostics | Process launch or terminal daemon |
| tuisnap-runtime | Piped process and PTY adapters, shell integration, lifecycle, event journal | CLI executable or another test runner |
| tuisnap | Public facade, Ratatui adapters, Insta macros, runner/test context | Heavy optional features in minimal configurations |
| tuisnap-cli | Capture, inspect, sessions, render, diff, review/report, migrations, agent protocol | Mandatory involvement in Rust tests |
| conformance fixtures | Nonpublished emitters, tiny Ratatui apps, negative tests, benchmark corpus | Production dependency graph |

These are dependency boundaries, not a demand to publish many user-facing APIs. Re-export through one facade and split only where it isolates dependencies or improves compilation. Keep tests in separate files. Use the repository's pinned tooling and normal logical commits. Breaking public APIs are acceptable; unreviewed changes to existing approved evidence are not.

### Data types

- ProcessOutput: separate stdout/stderr bytes, termination classification, bounded-output state, and diagnostic timing. No invented total ordering between independent stdout/stderr pipes.
- Screen: dimensions and origin, full row-major grid, source graphemes, explicit lead/continuation geometry, source color references, styles, cursor intent, and resolved color state needed for appearance-specific comparisons.
- Observation: an owned screen plus observed modes, clipboard/bells/title/links/graphics, optional application semantics, revision and capture reason, capability coverage, and diagnostics. Informational times/counters are not automatically baseline fields.
- TerminalProfile: advertised and observed terminal/protocol behavior. Do not advertise capabilities that the selected backend cannot implement.
- RenderProfile: font bytes/hashes, ordered fallback and style faces, geometry, scaling, palette resolution, cursor/blink phase, and renderer behavior version.
- ComparisonPolicy: distinguishes textual, styled-grid, terminal-state, rendered-pixel, and artifact-byte contracts. There is one implementation per contract, reused by tests and CLI.
- ArtifactSet: one candidate generation, checks performed, provenance, byte limits/truncation, and per-artifact hashes. A set is complete only after its completion manifest is published.

Unknown and Unsupported are not aliases for false, empty, or default. A required unsupported capability fails. An intentionally excluded capability is visible in the policy/report.

## Ordered work packages

### P0 — Verification correctness

| ID | Work | Acceptance criterion |
|---|---|---|
| C01 | Remove cell-equality bypass from pixel comparison | Same cells and dimensions with different decoded pixels fail strict image checks |
| C02 | Remove approved-to-actual image copying as verification evidence | Actual images come from rendering the candidate; skipped rendering is explicitly not_checked |
| C03 | Add exact decoded RGBA/opaque-policy comparison | Re-encoding identical pixels passes; one relevant channel difference fails; alpha semantics are explicit |
| C04 | Separate perceptual diagnostics from exact verdicts | A rounded similarity score cannot establish strict equality; invalid tolerances are rejected |
| C05 | Unify classic/grouped/test/CLI/report verdict evaluation | A report cannot disagree with the test because it compared compressed bytes or ignored a threshold |
| C06 | Make required missing/corrupt reference artifacts fail | No expected-image regeneration in frozen visual mode |
| C07 | Make assertions hard to ignore | Assertion API fails on mismatch; inspection outcomes are must_use; examples do not merely unwrap I/O success |
| C08 | Transactional candidate generation and consistent approval | Interrupted writes and partially accepted generations are rejected |
| C09 | Preserve historical references | Original artifact hashes stay unchanged through the tooling migration |
| C10 | Reconcile README and actual behavior | Claims about embedding, pixel checking, and approval regeneration match tested behavior |

### P1 — API, model, and direct views

| ID | Work | Acceptance criterion |
|---|---|---|
| M01 | Introduce validated immutable screen/observation model | One capture is one consistent revision; imported invalid data cannot create an invalid screen |
| M02 | Preserve all baseline-relevant source distinctions | Default/indexed/RGB colors, independent modifiers, styled blanks, continuations, cursor visibility/shape/blink intent survive |
| M03 | Extend observed state without bloating default snapshots | Underline style/color, hyperlinks, modes, title, bells, clipboard, palette changes and graphics can be asserted explicitly |
| M04 | Separate static dimensions from PTY limits | Valid one-row/one-column view fixtures work without inheriting emulator restrictions |
| M05 | Direct Ratatui production closure/buffer/TestBackend adapters | No child, controller invocation, service bootstrap, or synthetic ANSI round trip |
| M06 | Stateful view support and explicit cursor placement | Actual production render functions work even without implementing Widget |
| M07 | Explicit region and mask policies | Wide graphemes cannot be cut silently; masks preserve geometry and are recorded in evidence |
| M08 | Stable source identity and runtime metadata separation | Timestamps, PIDs, retry indices and absolute checkout paths do not become default approval keys |
| M09 | Minimal facade configuration | A pure view consumer compiles without runtime/native emulator/CLI dependencies |

### P2 — Insta and nextest vertical slice

| ID | Work | Acceptance criterion |
|---|---|---|
| I01 | assert_snapshot! for styled canonical state | Native Insta pending/review behavior, source locations, names/suffixes/settings and readable structural diff |
| I02 | assert_screenshot! for one visual sample | All actual evidence is generated before failure; canonical and PNG expectations bind to the same sample |
| I03 | Implement Insta custom pixel comparator | Uses public Comparator API; invalid image/policy never matches; ignores no unrelated canonical fields |
| I04 | Compound snapshot lifecycle | Partial acceptance of linked canonical/image expectations cannot produce a passing mixed baseline |
| I05 | Public review integration | No vendored Insta internals; propose upstream hooks if public APIs cannot publish/report a compound result cleanly |
| I06 | Distinguish evolving and frozen references | Evolving snapshots use review; frozen roots reject acceptance and never self-heal |
| I07 | Four-artifact export and read-only importer | Every visual sample emits ANSI/TXT/PNG/HTML from one generation; canonical state and PNG are default expectations, the other formats are evidence unless byte comparison is requested, and frozen four-file trees import/verify without changing |
| N01 | Plain libtest integration | No required custom proc macro or independent test runner |
| N02 | Stable test identity plus isolated attempts | Workspace/package/binary/test/scenario form baseline identity; run/attempt/stress/shard identify scratch artifacts |
| N03 | Runtime executable resolution | Prefer nextest-remapped executable paths; never silently use a stale build-time absolute path |
| N04 | Archive/remap support | Run from a relocated nextest archive without source-path or binary-path assumptions |
| N05 | JUnit/report correlation | Supplementary artifacts join nextest results without rewriting success/failure or inventing successful tests |
| N06 | Required scenario manifest | Full-suite gate detects required tests/captures not executed; filtered runs are visibly partial |
| N07 | Cancellation/timeout evidence | An absent completion marker is incomplete, not pass; last flushed journal remains inspectable |
| N08 | Retry/stress isolation | Failed-attempt artifacts survive later success; repeated stress runs cannot overwrite each other |
| N09 | Parallel consumer tests | Two nextest processes and multiple in-process tests do not share mutable env, global session names, ports, or candidate paths |
| N10 | Documentation example coverage | Examples execute as ordinary tests or through a separately verified doctest lane; no assumption of implicit coverage |

Native Insta binary storage is useful but its default comparator is byte-based. The current public Comparator supports custom behavior. A compound canonical-plus-PNG adapter still needs a real integration spike; ordinary macros do not automatically provide an atomic multi-artifact review transaction. [S8–S10]

### P3 — Native CLI and PTY runtime

| ID | Work | Acceptance criterion |
|---|---|---|
| R01 | First-class piped Command | Separate stdout/stderr, stdin EOF, nonzero exit, signals and timeouts are represented honestly |
| R02 | Deadlock-safe streaming | Both pipes are drained; output limits/spooling are explicit; non-UTF-8 bytes survive |
| R03 | Process fixtures and isolated environments | Temp HOME/XDG/cwd and child-only env changes; necessary dynamic-library paths can be preserved deliberately |
| R04 | PTY owner and emulator adapter | Reuse official upstream crates; no embedded competitor engine, vendored forks or repaired-source copies |
| R05 | Backend qualification | First full-fidelity candidate is the official Ghostty binding; promote only after capability/build/platform tests; no automatic substitution |
| R06 | Atomic observation and revisioned event loop | Grid/cursor/palette/modes captured together; read/compare does not create torn composite state |
| R07 | Deterministic scheduling of control operations | A long wait does not block cancellation, observation, or unrelated sessions |
| R08 | Owned cleanup with bounded shutdown | Normal return, error, panic and cancellation reap owned processes and close handles without double panic |
| R09 | Abrupt-parent-death protection | A scoped guardian/platform process containment handles supported hard-kill cases; escaping descendants are an explicit boundary |
| R10 | Separate readiness/stability/frame/exit waits | No generic idle check falsely establishes business completion; synchronized-frame support is capability-gated |
| R11 | Complete input surface | Text, typed chords, raw bytes, press/down/repeat/up, negotiated paste, click/hover/drag/wheel/focus/resize/signal |
| R12 | Shell sessions and command markers | Explicit shell launch only; last shell-command exit is not direct-child exit; integration availability is reported |
| R13 | Terminal state assertions | Palette/default colors, clipboard, bells, title, modes, scrollback and links have explicit assertions |
| R14 | Safe bounded raw replay | Same bytes under arbitrary chunks give the same state; replayed input is never mistaken for terminal output |

The Ghostty choice is a candidate engineering recommendation, not an assertion that it has passed the full required corpus. Native build dependencies must never reach direct view users. Optional backends remain useful for compatibility tests, but must report capability differences rather than silently normalize them away. [S5–S7, S17]

### P4 — Playwright-style queries and assertions

| ID | Work | Acceptance criterion |
|---|---|---|
| Q01 | Fresh text/regex/style/link/region locators | Queries resolve against current revision with documented exact/substring/normalization behavior |
| Q02 | Composition and scope | within/before/after/nth/first/last/and/or/filter operate on terminal cell spans and logical lines correctly |
| Q03 | Strict actions | Multiple matches fail unless explicitly disambiguated; scrollback text is not clicked as viewport content |
| Q04 | Retryable observational assertions | Visibility/text/style/count checks retry to one deadline; permanent usage/unsupported errors fail immediately |
| Q05 | Actions execute once | Retrying locator readiness never repeats destructive click/submit actions implicitly |
| Q06 | Optional semantic provider | Role/id/label/focused/disabled/hit-region come only from an explicit provider, not guessed ASCII styling |
| Q07 | Real input for semantics-assisted E2E | A semantic locator chooses a real input target; it does not call application controllers directly |
| Q08 | Negative temporal assertions | not_present_now, eventually_absent, and remains_absent(duration) have distinct tests |
| Q09 | Event/clock harness for deterministic runtime tests | Caller-supplied event/reducer/render hooks; no imposed product architecture or live-service requirement |
| Q10 | Region/palette/cursor invariants | Region comparisons retain origin and wide-cell policy; appearance checks resolve the recorded palette |

### P5 — Rendering and visual evidence

| ID | Work | Acceptance criterion |
|---|---|---|
| V01 | Renderer independent of process capture | Saved/direct/live screens use the same rendering API |
| V02 | Explicit font packs | All style/fallback face bytes hashed; no system scan in deterministic mode |
| V03 | Grapheme shaping qualification | Combining sequences, CJK, Nerd icons, box drawing, Braille, blocks, emoji policy and clipped styles tested |
| V04 | Source widths control layout | Rendering/fallback cannot shift the canonical terminal grid |
| V05 | Strict missing-glyph policy | Fail by default in visual approval mode; placeholder permitted only explicitly and reported |
| V06 | Raw state versus display versus safe export | Concealment is not redaction; sensitive source data cannot leak through hidden JSON or trace sidecars |
| V07 | Blink intent versus sampled phase | State retains requested behavior; still image uses a declared deterministic phase |
| V08 | Content-addressed caches | Key includes all relevant data/font/profile/renderer versions; corrupted cache rejected; qualification has no-cache mode |
| V09 | Portable reports | Complete offline bundle; expected/actual/overlay/diff, fields, profile, action context, capability coverage |
| V10 | Byte-sensitive contracts remain possible | Canonical ANSI/TXT/HTML byte comparison is opt-in when representation bytes themselves are contractual |

### P6 — Competitor feature parity and advanced use

| ID | Work | Acceptance criterion |
|---|---|---|
| A01 | One typed operation/result protocol | Rust API, CLI and machine protocol share validation and error meanings |
| A02 | Optional named sessions | Versioned endpoints, isolated names, safe restart/stop/prune, owner-only runtime directories |
| A03 | Live human inspection | Observe/interact with the same owned session without requiring a multiplexer |
| A04 | Trace journal and viewer | Input, delivered bytes, frame revisions, waits, assertions, exits and artifact status are correlated |
| A05 | Replay boundaries | Offline observation replay is distinguished from rerunning a nondeterministic real application |
| A06 | Record and export | Lossless bounded event recording; deterministic screenshot/cast/GIF/APNG exports; MP4 via optional external encoder |
| A07 | Graphics protocols | Payload inspection first; composited-image equality only once specifically implemented and qualified |
| A08 | Agent CLI and MCP | Schema, capabilities, inspect, action, assertions, artifacts; no host clipboard or filesystem access via terminal escapes by default |
| A09 | Optional non-Rust clients | Thin clients over the same protocol; no second assertion engine; not mandatory for Rust consumers |
| A10 | Migration adapters | Current tui-snap stores and selected competitor traces accepted with explicit unsupported-field reports |
| A11 | Performance qualification | Published cold/warm build, startup, capture, render, memory and full-suite measurements with hardware/corpus/toolchain |
| A12 | Cross-platform conformance | macOS/Linux required; Windows coverage records ConPTY differences; supported subset is explicit |

## CLI responsibility

Keep tuisnap for capture/inspect/session control, saved-frame rendering, diff, review/report, replay, schema/doctor, and migrations. Do not implement another Rust test scheduler. Optional tuisnap test would only invoke nextest transparently, preserving its exit status and configuration; it is not necessary for the recommended workflow.

Suggested learning order: pure view example; styled snapshot; screenshot; piped CLI example; interactive TUI example; artifacts and review; locators; timing/cancellation; advanced profiles; agent protocol. Avoid making users learn process daemons or backend traits before rendering one widget.

## Build/release gates

1. P0 verification mutations pass before refactoring storage or capture.
2. A pure view plus Insta plus nextest vertical slice works before broad agent functionality.
3. Piped CLI and PTY paths pass independent lifecycle/terminal oracles.
4. Original frozen approvals remain unchanged while capture and model adapters migrate.
5. A renderer migration, where required, has a separate profile/version and reviewed evidence from the historical reference—not from the current application.
6. Competitor replacement is claimed only for the shipped capability matrix. Deferred bindings, graphics compositing or video features are not counted as implemented.
7. No unsafe cross-thread terminal sharing, no third-party source forks, no hidden backend fallback, no baseline auto-accept in CI, no successful status for absent captures.
8. CI fast-lane target: under 120 seconds on declared hardware/cache assumptions. Optimize and shard rather than removing fidelity checks. This is a target, not a measured result.

## Reproducible negative tests

Use a small hand-authored VT fixture app and real production Ratatui fixtures. Add mutants for a changed symbol/style/continuation/cursor, one changed pixel, missing font, modified palette, concealed-data leak, deleted baseline, partial approval, wrong binary path, duplicate snapshot identity, hanging stdin, full stdout+stderr pipes, late output after direct process exit, cancellation while waiting, parent hard kill, stale semantic revision, repeated destructive action, skipped scenario, and retry overwriting failure evidence.

Differential comparison with competitors is useful but not the oracle: two engines can agree on the same omission. Preserve unsupported observations and define policy decisions rather than normalizing every disagreement until tests pass.

## Source references

[S1] tui-snap README: https://github.com/tailrocks/tui-snap/blob/9dc86daff1dcbf20805b145916e8f04e9515f929/README.md
[S2] tui-snap current frame schema: https://github.com/tailrocks/tui-snap/blob/9dc86daff1dcbf20805b145916e8f04e9515f929/src/frame.rs
[S3] tui-snap pixel comparator: https://github.com/tailrocks/tui-snap/blob/9dc86daff1dcbf20805b145916e8f04e9515f929/src/diff.rs
[S4] tui-snap grouped check/report/accept: https://github.com/tailrocks/tui-snap/blob/9dc86daff1dcbf20805b145916e8f04e9515f929/src/grouped.rs
[S5] tui-test feature/API overview: https://github.com/microsoft/tui-test/blob/7afb14b3c4075d24a7b9bf1a05175717f253821c/README.md
[S6] tui-test snapshot serializer: https://github.com/microsoft/tui-test/blob/7afb14b3c4075d24a7b9bf1a05175717f253821c/crates/tui-test/src/assert/snapshot.rs
[S7] Terminal Control features: https://github.com/anomalyco/terminal-control/blob/c1d4f95e4f1b7638f6229e9bfc9599a955f95ce2/README.md
[S8] Insta Comparator: https://github.com/mitsuhiko/insta/blob/064742e9b7b2f3eaabb4724069e739ddf23d8227/insta/src/comparator.rs
[S9] Insta update policies: https://insta.rs/docs/advanced/
[S10] Cargo Insta review and nextest runner: https://insta.rs/docs/cli/
[S11] nextest runtime identities and remapped binaries: https://nexte.st/docs/configuration/env-vars/
[S12] nextest leak detection limits: https://nexte.st/docs/features/leaky-tests/
[S13] nextest test groups: https://nexte.st/docs/configuration/test-groups/
[S14] nextest retries/flaky-result: https://nexte.st/docs/features/retries/
[S15] nextest JUnit: https://nexte.st/docs/machine-readable/junit/
[S16] nextest stress tests: https://nexte.st/docs/features/stress-tests/
[S17] Terminal Control native dependencies: https://github.com/anomalyco/terminal-control/blob/c1d4f95e4f1b7638f6229e9bfc9599a955f95ce2/Cargo.toml
[S18] Terminal Control semantic protocol: https://github.com/anomalyco/terminal-control/blob/c1d4f95e4f1b7638f6229e9bfc9599a955f95ce2/docs/semantic-protocol.md
[S19] Terminal Control extractor: https://github.com/anomalyco/terminal-control/blob/c1d4f95e4f1b7638f6229e9bfc9599a955f95ce2/src/terminal_core.rs
[S20] Terminal Control renderer: https://github.com/anomalyco/terminal-control/blob/c1d4f95e4f1b7638f6229e9bfc9599a955f95ce2/src/render.rs
