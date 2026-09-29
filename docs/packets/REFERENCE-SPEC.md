# Tuiscotti: PR #6 corrective architecture and implementation specification

**Status:** proposed corrective specification; not implemented or benchmarked by this review.
**Review date:** 2026-09-29.
**Current repository:** `tailrocks/tui-snap`, PR #6, branch `redesign/rust-first-testing-platform`.
**Inspected PR head:** `7e8272bc08dd3241d731f832c573fd4c6de3fe1e`.
**Inspected termpane head:** `8ff87fe1795b5a246214e9dc8a2a000c8746dab5`.
**Proposed product identity:** **Tuiscotti — Rust TUI visual-regression toolkit**.

This document and `IMPLEMENTATION-GOAL.txt` address the user's eight corrective goals. They supersede conflicting earlier directions to choose Alacritty/Ghostty as the product backend or ship Python/JavaScript clients. The still-relevant three-mode, Ratatui, Insta, nextest, fidelity, lifecycle, and immutable-reference requirements remain in force.

## 0. Review findings and evidence limits

The PR is now an implementation, not the documentation-only revision reviewed earlier. Its metadata reports 52 commits and 151 changed files. The author claims 82/82 completion and several successful test runs; those are reported claims, not results reproduced in this review. This review inspected the current manifests, public entry points, runtime declarations, examples, assertion implementation, CLI parser, PR discussion, and current termpane interfaces. A container checkout failed because DNS/network access was unavailable. No Rust build, runtime test, benchmark, or end-to-end migration was executed. [R1]

| Finding | Inspected evidence | Consequence |
|---|---|---|
| Wrong direct backend dependencies | Root `pty = ["dep:portable-pty", "dep:alacritty_terminal", "dep:libc"]` | G1 requires replacing this boundary, not just renaming imports. [R2] |
| Current backend is not fully equivalent | `TerminalProfile::check` rejects per-cell blink and synchronized output | Returning `Unsupported` is honest, but does not prove required capability completion. [R3] |
| Current termpane has no process/PTY API | Model-only README, manifest with vte/Unicode/storage crates, public model exports | Upstream process/transport work is required before the requested replacement can run. [R4–R6] |
| New scope is contradicted by shipped clients | PR lists `clients/py` and `clients/ts`; README still invokes Python migration tooling | Remove implementation, packaging, execution hooks, and active docs together. [R1,R7] |
| Workspace extraction was not completed | Single root package, edition 2021, root `src/` and `examples/` | Build a real virtual workspace and enforce inheritance. [R2,R8] |
| Low-level API leaks into ordinary examples | View quickstart requires provenance/profile/store; locator example constructs observations manually | Replace the normal path with one facade and bound locators; preserve advanced primitives separately. [R7,R9] |
| CLI corrupts the child argument boundary | `extract_flag` removes every `--machine` from all argv before Clap | `capture -- app --machine` incorrectly selects the parent's machine mode. This is a source-derived counterexample, not an executed test. [R10] |
| CLI loses native argument representation | `std::env::args()` and `Vec<String>` for child argv | Use `args_os`, `OsString`, `PathBuf`, and Clap-native parsing. [R10] |
| Insta metadata integration is incomplete | `assert.rs` explicitly says `.snap source` is `src/assert.rs`, not the caller | Fix native call-site metadata; a prose description is not a substitute. [R11] |
| Sample identity is narrower than its purpose | `generation_id` hashes canonical text only | Bind render profile, font identities, policy/schema and actual artifact digests as needed; test same-screen/different-render consistency. This is a design risk requiring a reproducer, not a proven exploit. [R11] |
| Binary resolution may choose an unintended build | Resolver searches both debug/release locations and returns the first existing candidate | Use runner/build metadata; stale explicit metadata must not silently fall back. [R12] |
| Documentation contradicts itself | PR body describes sparse additive schema-v3 fields; README calls them schema v4; legacy store APIs remain public | Establish the actual wire format from code and compatibility tests, then write one authoritative contract. [R1,R7,R8] |

Do not carry forward old bug claims without retesting them: the current PR claims to have corrected pixel bypasses and approval consistency. Preserve its real improvements, add adversarial proof, and fix only verified current weaknesses. The old “82/82” ledger must be re-audited against the new scope; do not merely change its branding.

## 1. Goal G1 — termpane is the only terminal backend boundary

### 1.1 Required architecture

```text
Tuiscotti terminal test / CLI session
    -> Tuiscotti test orchestration, queries, assertions, journals
    -> termpane's safe process/PTY and terminal-state APIs
    -> upstream OS/PTY primitives privately owned by termpane

Raw VT replay -> termpane::DamageGrid / validated snapshots
Pure Ratatui views -> Tuiscotti passive Screen model (no process/PTY)
Piped CLI -> std::process plus termpane process-supervision APIs where needed
```

Tuiscotti must have **no direct dependency, renamed dependency, re-export, feature edge, build dependency, dev dependency, or source use** of `portable-pty`, `alacritty_terminal`, or `libc`. No local second emulator, secondary ANSI shadow parser, vendored termpane/termlens, private fork, or optional fallback backend.

This is a boundary requirement, not a claim that an entire Rust dependency graph contains no OS implementation crates. `libc` can appear transitively through ordinary upstream libraries. If termpane chooses `portable-pty` privately for its optional transport implementation, Tuiscotti still depends only on termpane at that boundary. Do **not** relocate Alacritty wholesale inside termpane: its existing DamageGrid remains the one terminal-state engine. Inspect and report the complete resolved graph rather than promising zero transitive `libc`.

The pure, deterministic termpane model must retain its current no-host-effects/default-feature behavior. Add optional functionality without requiring every terminal-model consumer to launch processes or acquire OS/native dependencies. Termpane's current first-party `unsafe_code = "forbid"` should remain intact: use safe upstream wrappers for OS primitives, not new local FFI exceptions. [R4–R6]

### 1.2 Existing capabilities versus upstream work

| Required surface | Evidence at inspected termpane head | Upstream work / acceptance |
|---|---|---|
| VT processing | Persistent vte parser and DamageGrid | Reuse; prove split-byte/UTF-8/escape invariance with the consumer corpus. |
| Cells and colors | Typed modifiers, source colors, underline styles/colors, hyperlink metadata | Add any missing public observation accessors, not consumer-side reconstruction. |
| Unicode/continuations | Grapheme/wide-cell handling documented | Qualify exact required cases and styles; do not assume all Unicode is perfect. |
| Cursor/modes/replies | Cursor/grid/mode/reply APIs exist | Inventory full cursor/blink/default/palette state and capability reporting; add missing data upstream. |
| Atomic snapshot | Owned GridSnapshot and borrowed views exist | Confirm one consistent revision includes all required state, including non-cell changes. |
| Synchronized updates | Not proven by this review | Reproduce DEC 2026 behavior and add completed-frame observation if missing. |
| PTY launch/read/write/resize | Absent from current public API | Add optional safe, bounded PTY transport. |
| Process lifetime/signals | Absent as a process API | Add owned exit/reap/terminate and safe identity/containment primitives. |
| Bounded cancellation and EOF | Not a model-layer responsibility today | Add transport semantics, final-output drain, cancellation and explicit incomplete outcomes. |
| Mode-aware input encoding | Observation exists; complete encoder unverified | Expose or add key/mouse/paste/focus encoding based on the same state. |
| Headless raw replay | Existing model accepts bytes without PTY | Preserve direct replay, never spawn a terminal merely to parse input. |
| Registry distribution | Package version says 0.1.0; registry publication not established | Publish/qualify the necessary official release before final consumer migration. |

### 1.3 Upstream PR sequence

**TP1: contract and conformance.** Inventory every Tuiscotti use of the removed dependencies. For each operation record the existing termpane API, missing behavior, independent reproducer, intended feature, and public test. Include the macOS lifecycle/guardian issue disclosed in PR #6 instead of assuming it is fixed.

**TP2: optional transport and process support.** Proposed feature names are `process` and `pty`, with `pty` depending on `process`; names are design choices, not existing termpane APIs. Preserve `default = []`. A safe reusable API owns argv/environment/cwd, reader/writer handles, child exit/reap, resize, signals, cancellation, drain status, and bounded shutdown. Keep test assertions, Insta, screenshot rendering, and nextest-specific metadata out of termpane.

**TP3: observation and input gaps.** Extend native termpane state for any missing cursor, palette, hyperlink, key/mouse protocol, frame completion, or damage revision behavior. No silent `false` for unknown state. Tests must include non-cell changes, both bold and dim, blink, styled blanks, wide followers, resets, scrollback, and alternate-screen transitions.

**TP4: official release and consumer switch.** Test an actual downstream consumer using the published package, then replace the PR runtime adapter and remove direct backend/system imports. Keep one branch/PR per repository; do not copy termpane source into Tuiscotti.

The supplied `deny.toml` explicitly rejects unknown Git sources and has `allow-git = []`. Therefore a pinned Git dependency is **not** a compliant final solution. The final dependency must be a real official registry release with Cargo.lock. Local workspace paths are for this project's own crates, not a permanent external termpane checkout. Missing release permission is an external completion blocker to document, not permission to broaden the allowlist.

### 1.4 Proof required

Run legacy-versus-new consumer tests with unchanged approvals, then remove the legacy execution path. Test parent death, child exit with late output, cancellation while waiting, blocked input, no read progress, resize during output, interrupt/terminate/kill distinctions, and owned descendants. State the supported process-containment boundary; do not claim RAII catches `SIGKILL` or that one system `ps` format is portable.

## 2. Goals G2 and G3 — Rust and CLI only; Rust maintenance tooling

### 2.1 Remove, rather than hide

Delete `clients/py`, `clients/ts`, their manifests, locks, examples, tests, generation hooks, documentation promises, packaging/release jobs, language setup actions, and publishing credentials references. Do not retain stub clients or move their code into an ignored directory. A short future-directions paragraph may say that foreign-language clients are out of current scope.

Remove all first-party Python, TypeScript and JavaScript scripts, including unchanged legacy files not shown in the PR diff. Search shebangs, extensionless files, build.rs, test string literals, documentation commands, workflows, Makefiles and task definitions. Do not smuggle scripts through `python -c`, `node -e`, Rust strings, generated `.js`, or inline HTML `<script>` elements.

Static HTML/CSS reports are fully supported. Use pre-rendered panels, images, anchors and HTML disclosure elements; advanced interactive review can be a Rust CLI/TUI rather than a JavaScript web app. Preserve overlay/diff inspection and accessible text output.

Externally installed tools may internally use other languages. Renovate is an external service/tool; JSON/JSON5 configuration is data. This does not authorize first-party JS validation scripts, a Node development environment, or a client package in the repository. External GitHub Actions implementations are not copied into this repository.

Keep useful Rust-owned CLI machine output, schemas and optional MCP/session functionality. Removing SDKs is not authorization to discard Rust or CLI coverage. Do not introduce a new remote client SDK under another name.

### 2.2 xtask replacement plan

Create `crates/xtask` with no dependency on the product facade, renderer or PTY runtime. Use focused Rust modules and thin subcommands:

| Maintenance task | Rust replacement |
|---|---|
| Migration/approval verification | Parse supported formats, validate conversion receipts and compare immutable hashes. |
| Fixture generation/exports | Build/resolve Rust fixture executables once; run deterministic export checks. |
| Repo shape/branding/dependencies | Cargo metadata and typed file checks; emit actionable diagnostics. |
| Source/function-size checks | Alint plus Clippy; xtask tests that these checks reject violations. |
| Docs/link/example checks | Rust file/path parsing and executable examples. |
| Performance collection | Rust timers/process results and machine-readable measurements, not Python scripts. |
| Font maintenance | Preserve current font bytes and notices; use a qualified Rust subsetting path when needed, or ship unchanged verified upstream font assets. Do not shell out to Python/fonttools. |
| CI generation | Invoke the authoritative generator; do not reimplement Velnor or patch generated YAML. |

Use `mise run <task>` to invoke `mbx run --package xtask -- <task>`. `cargo xtask` remains the conventional Rust task pattern; an optional Cargo alias can be provided, but documented CI/development compilation must pass through mbx. Verify custom-subcommand forwarding with the selected mbx release. Never recursively invoke the same xtask command from itself.

Do not replace every Python script with a large shell script. Shell-based fixture applications should become Rust fixtures; retain only minimal command orchestration where a platform CI runner requires it.

## 3. Goal G4 — Documentation that describes the shipped product

### 3.1 Canonical document map

```text
README.md                          identity, install, three short workflows, limits, links
AGENTS.md                          short enforceable contribution rules
CONTRIBUTING.md                    Mise/mbx/xtask, lints, tests, PR expectations
CODEOWNERS                         actual maintainers; do not guess account identities
CHANGELOG.md                       intentional API/backend/format changes

docs/
  architecture.md                  model, termpane boundary, dependency graph, lifecycle
  testing.md                       views vs pipes vs PTY, fixtures, retries, negative tests
  snapshots.md                     comparisons, profiles, Insta, formats, frozen evidence
  api-design.md                    public surface and current/target syntax decisions
  cli.md                           generated reference + task-oriented examples
  comparisons.md                   detailed revision-pinned competitor comparison
  tooling.md                       Rust/lints/mbx/Mise/xtask/Renovate/CI contracts
  performance.md                   measured workloads and build boundaries
  migration.md                     one deliberate migration; no live obsolete API aliases
  limitations.md                   observed backend/platform/export limitations
  decisions/                      only durable decisions with code/test references
  history/                        frozen historical research, not active guidance
```

Consolidate duplicated root/docs research files. Do not just move stale statements into a new path. Reconcile actual frame schema/version, current ownership, exported names, reported tests, report embedding and supported operations. Replace “82/82 done” with an evidence ledger whose removed client/backend items are explicitly **superseded by this request**, not falsely still implemented.

### 3.2 Comparison requirements

Compare Tuiscotti with `microsoft/tui-test`, `anomalyco/terminal-control`, and `vyncint/termlens`. Pin code revisions and retrieval dates. For every cell use **implemented and tested**, **implemented but not independently qualified**, **partial**, **unsupported**, **out of product scope**, or **proposal**. Do not conflate installed plugins, open PRs or doc examples with shipped behavior.

Compare public Rust APIs and CLI workflows separately: view rendering, piped processes, terminal sessions, binary resolution, environment isolation, waits, input, locators, semantic annotations, negative assertions, state completeness, screenshots, artifact-byte checks, approvals, native Insta, nextest, traces, lifecycle, platform support and build cost. Existing competitor strengths deserve recognition: tui-test binds lazy queries to live sessions; termlens has compact builder/wait APIs; Terminal Control exposes detached frames/rendering. [R13–R15]

The comparison must include runnable examples or narrowly marked schematic examples, semantic differences, required types/imports, failure modes, and a specific Tuiscotti change. Do not declare a winner solely by counting source lines.

## 4. Goal G5 — Real Rust fixtures and every output format

### 4.1 Layout resolving both path requirements

All first-party Rust stays under `crates/`. The requested `tests/fixtures` tree is **crate-relative**, not root-level Rust source:

```text
crates/tuiscotti-fixtures/
  Cargo.toml                       publish = false; explicit [[bin]] targets
  src/lib.rs                       reusable real view functions + deterministic model types
  src/views/                       focused view implementation modules
  tests/fixtures/apps/menu.rs       [[bin]]: interactive Ratatui application
  tests/fixtures/apps/streams.rs    [[bin]]: stdout/stderr/stdin/exit fixture
  tests/fixtures/apps/protocol.rs   [[bin]]: terminal protocol and lifecycle emitter
  tests/fixtures/data/             checked input data
  tests/fixtures/expected/         approved evidence / compatibility corpus
  tests/format_contracts.rs
  tests/view_contracts.rs
  tests/interaction_contracts.rs
```

Fixture application source excluded from a generic line-count glob must still be small and reviewed. Never hide product implementation in fixtures to avoid a limit. Benchmarks/examples and xtask source also live under `crates/`.

Compile fixture binaries once through the outer build/nextest process. Resolve the selected Cargo package/target using supported metadata. Do not run nested `cargo build` in every test or launch an arbitrary stale `target/debug` binary. [R16]

### 4.2 ASCII is not ANSI

Support the user's wording without losing earlier requirements:

| Format | Contract |
|---|---|
| `ansi` / `.ansi` | Normalized VT/SGR screen export. Not the original byte transcript. |
| `txt` / `.txt` | Plain Unicode text, no terminal escape sequences, documented trailing-space policy. |
| `ascii` / `.ascii` | Explicit 7-bit diagnostic projection, documented substitutions and a loss report. No claim of lossless Unicode fidelity. |
| `png` / `.png` | Independently rendered pixels using a pinned profile and declared alpha handling. |
| `html` / `.html` | Offline static review artifact; no JS; escaped metadata and source content. |
| `json` / canonical screen | Versioned full-state evidence used to explain and verify the visual sample. |

Retain both ASCII and ANSI. Never silently substitute one for the other. ASCII output is a requested projection; lossy substitutions cannot satisfy the canonical Unicode snapshot check.

### 4.3 Required fixture matrix

Test pure production views and the real application using the **same view function**. Test state transitions separately rather than invoking controllers from view tests.

Cover three sizes including one-row/one-column static fixtures, dark/light palettes, RGB/indexed/default colors, simultaneous bold/dim, underline styles/colors, blink intent, cursor visibility/shape, styled blanks, wide continuations, combining sequences, CJK, Nerd icons, box drawing, clipping, focus/selection/disabled/error/empty states, resize, paste, keyboard/mouse/focus and independent stdout/stderr bytes.

For every required format assert content/signature, deterministic behavior, truncation/loss policy, path/identity, escaping, and deliberate failure behavior. Include PNG recompression versus pixel changes, ASCII non-ASCII substitutions, ANSI style-only changes, plain text trailing spaces, hidden data export, HTML injection strings, corrupt/missing references and cross-generation artifact mixes.

All format artifacts for a capture must identify the same source revision. Export errors cannot be ignored or replace the original application/test error. Freeze existing expectations; moving files is allowed with a verified path/hash mapping, not mass regeneration.

## 5. Goal G6 — Canonical Rust API and CLI redesign

The references are Rust API Guidelines, current Rust/Cargo documentation, and relevant primary project sources—not the assumption that one organization's house style is universal. Builders can validly consume or borrow; choose by semantics and be consistent. Prefer std-like reusable `&mut self -> &mut Self` setters for command/session launch descriptions. Side effects happen at an explicit fallible terminal method. [R17–R19]

### 5.1 Detailed syntax audit and target decisions

| Surface | Current PR issue / comparison | Target and acceptance |
|---|---|---|
| Imports | Core types are re-exported, but `Tui`/`Command` and internals remain spread across modules | Deliberate facade exports `Tui`, `Command`, `Screen`, `Error`, `Result`; no public worker/protocol implementation modules. |
| Program construction | `Tui::new(["./app"])` array mixes program and argv | `Tui::new(program).args(...)`; `AsRef<OsStr>`/`OsString` preserve native arguments. |
| Cargo binary | String failure and heuristic first-found search | Fallible typed resolution for explicit package/target; selected runner metadata authoritative; ambiguous/stale mappings fail. |
| Pure render | Four-argument `render_screen`, manual policy and conversions | `ratatui::render((cols, rows), draw)?` with safe default; advanced options through a clear separate builder. |
| Screen vs Frame | Public conversion helpers and two active models | One validated public `Screen`; isolate historical wire DTOs in read-only import. Render accepts `&Screen`. |
| Error types | `Result<PathBuf,String>`, auxiliary spawn-error field | `Result<T, Error>` with typed sources and context; spawn/IO errors are errors, nonzero child exit is a truthful output status. |
| Paths/bytes | CLI `String` argv; lossy stdout helpers prominent | Native paths/args; raw stdout/stderr accessors; `stdout_str()` is fallible and `stdout_lossy()` explicitly lossy. |
| Wait configuration | Ordinary examples pass `Instant::now()+...` and cancellation tokens | Session defaults; `.wait_for_text(...)`; per-operation `.timeout(Duration)` advanced options; no hidden infinite wait. |
| Cancellation | Manual token on every basic operation | Scoped session ownership with explicit optional cancellation handle; waits do not block stop/observe. |
| Locators | Example manually supplies revision and observer closure | Session-bound lazy locator, e.g. `app.get_by_text("Settings")`; advanced pure query evaluation still available. |
| Actions | Query results risk stale-coordinate coupling | Unique fresh target; never click scrollback; retry readiness, not destructive input; stale semantic revision rejected. |
| Input | Separate low-level methods and parsed strings | Typed `Key`/modifiers plus parsed CLI chords; fallible parsing via `FromStr`; literal bytes and paste remain explicit. |
| Negative checks | Boolean helpers can obscure temporal meaning | Distinct absent-now, eventually-absent, remains-absent operations; targeted assertions return a typed error. |
| Snapshot macros | Source field identifies library code | Expand supported Insta assertions at caller; `$crate` hygiene, evaluate input once, correct file/module/test identity; verify with an external consumer. |
| Capture options | Normal user must understand provenance/storage machinery | `snapshot()` uses documented default policy; immediate `observe()` distinct; advanced mode explicitly selected. |
| Render configuration | Font constants/profile construction exposed too early | Versioned default font pack; optional `RenderOptions`; strict custom packs require explicit data/hashes, no system lookup. |
| Comparison | Different stores expose similar overlapping APIs | One engine and verdict type; convenience assertion macros plus fallible inspection; no new redundant store hierarchy. |
| Termination | SpawnError encoded as a run status | Separate launch error from `Exit`/`Signal`/`Timeout`/`OutputLimit` and retain incomplete-stream evidence. |
| Cleanup | Drop joins/detaches with incomplete portable guarantees | Bounded explicit `finish`/`close` plus nonpanicking Drop; termpane owns OS lifecycle primitives; supported guardian behavior tested. |
| Traits | Risk of traits/generics proliferating during split | Concrete common types; `From` only infallible, `TryFrom` validated, `AsRef` for borrowed data; extension traits only at meaningful policy boundaries. |
| State encapsulation | Model/public internals can permit invalid construction | Private invariant-bearing fields, validated constructors, read-only accessors; meaningful Debug with secrets redacted. |
| Extensibility | Temptation to add another backend plug-in | Renderer/comparator/safe artifact-sink extension points allowed; terminal backend is termpane only, no public fallback selector. |
| Synchrony | Simple Rust tests should not acquire async runtime | Blocking facade is first-class; add async adapters only for a demonstrated Rust need, without forcing Tokio on pure views. |
| File structure | Monolithic root modules | Responsibilities in crates; no giant umbrella generic function; private helpers with purposeful names and strict limits. |

Use neither "every setter returns Result" nor "every failure becomes stored state until much later" indiscriminately. Validate cross-field launch/render settings once before side effects, while parsing/resolution functions that have already performed fallible work return errors at that boundary. [R19]

### 5.1a Concrete peer syntax comparison

The snippets in the first four columns describe the inspected interfaces, not assertions that all four implementations have identical semantics. The last column is proposed Tuiscotti syntax. [R7,R9,R13–R15]

| Task | Current PR | tui-test Rust | Terminal Control Rust | termlens Rust | Proposed Tuiscotti |
|---|---|---|---|---|---|
| Launch description | `Tui::new(["./app"]).size(120, 40).spawn()?` | `Session::new(name)`, then `open(options)` or a typed run operation | `Session::start(&argv, cwd, record, &options)?` | `Terminal::builder().size(80, 24).timeout(...).spawn(path)?` | `Tui::new(path).args(args).size(120, 40).spawn()?` |
| Content readiness | Manual predicate, `Instant` deadline and cancel token | `session.get_by_text("Ready").expect()?` | `session.wait_for_text("Ready", timeout)?` | `terminal.wait_until(|s| s.contains("Ready"))?` | `app.get_by_text("Ready").expect_visible()?` with session default and explicit per-call overrides |
| Locator ownership | Caller-supplied observer and revision in example | Lazy locator stores its session owner; composition checks owner identity | Documented example uses direct text waits and coordinate mouse input | Screen predicates and matching accessors | Bind live queries to a session; retain a separately useful pure evaluator |
| Settled capture | `wait_stable(deadline, &cancel)` plus screen/frame adaptation | Observation, screenshot and recording operations are exposed separately | `session.capture(settle, deadline)?.shot` returns a capture reason too | `wait_stable(quiet)?` or `snapshot_after(predicate)?` returns one screen | `app.snapshot()?`; advanced capture options expose stability/deadline and explicit incomplete state |
| Pure views | `render_screen(cols, rows, draw, edge_policy)` or `draw_frame` | Not the first-class direct fixture workflow in the reviewed Rust examples | Detached frame rendering is available, not a packaged production-view adapter | Recommends using Ratatui TestBackend alongside PTY tests | `ratatui::render(size, draw)?`; advanced view options remain available |
| Snapshot review | Macros and two overlapping store APIs | Built-in `.snap` operations rather than native Insta workflow | Documented client workflow supplies separate snapshot testing | `assert_screen_snapshot!(terminal, after = predicate)` uses Insta | Native Insta macros with correct source location and complete visual-sample consistency |
| Exit and cleanup | Separate session close/finish and status models | Session close plus exit operations | `wait_for_exit(timeout)?` and `stop()?` | `wait_exit()?` and managed terminal lifetime | Explicit status assertions and bounded finish/close; no hidden panic on teardown |

The recommended design borrows termlens's concise defaults and tui-test's bound locator ownership. It keeps Terminal Control's useful explicit capture reason and detached rendering, without requiring its positional launch parameter list. It does not copy a framework's snapshot semantics merely to mimic method names.

### 5.2 Intended ordinary Rust experience (proposed, not shipped)

```rust
#[test]
fn settings_view() -> tuiscotti::Result<()> {
    let model = fixtures::settings();
    let screen = tuiscotti::ratatui::render((100, 30), |frame| {
        fixtures::render_settings(frame, &model);
    })?;
    tuiscotti::assert_snapshot!("settings-state", &screen);
    tuiscotti::assert_screenshot!("settings-image", &screen);
    Ok(())
}

#[test]
fn navigation() -> tuiscotti::Result<()> {
    let mut app = tuiscotti::Tui::cargo_bin("menu-fixture")?
        .size(100, 30)
        .spawn()?;
    app.get_by_text("Ready").expect_visible()?;
    app.press("Ctrl+P")?;
    app.get_by_text("Settings").click()?;
    let screen = app.snapshot()?;
    tuiscotti::assert_screenshot!("settings-open", &screen);
    app.press("q")?;
    app.expect_exit().success()?;
    Ok(())
}
```

`cargo_bin` must not compile a program. Keep the real artifact identity from runner metadata. Allow direct `Tui::new(path)` when the caller already knows the executable. The names above express target ergonomics; implement and compile them or document an evidence-backed improvement rather than marketing nonexistent methods.

### 5.3 CLI redesign

Use Clap derive with explicit command types, typed format enums, `PathBuf`, `OsString`, `args_os`, tested conflict/value handling, and complete generated help. Remove `extract_flag` and hidden pre-parsing. Prefer an explicit `machine` subcommand for JSON Lines and ordinary `--json` output where applicable.

Proposed coherent vocabulary:

```text
tuiscotti init
tuiscotti doctor
tuiscotti capture --mode tui --format ansi,txt,ascii,png,html --out artifacts/home -- app --machine
tuiscotti capture --mode cli --out artifacts/cli -- app --help
tuiscotti inspect artifacts/home
tuiscotti render screen.json --format png,html --out artifacts/rendered
tuiscotti diff expected.png actual.png
tuiscotti verify --reference snapshots --actual artifacts
tuiscotti review artifacts
tuiscotti accept --candidate <generation> --store snapshots <scenario>
tuiscotti report artifacts --out report.html
tuiscotti session start demo -- app
tuiscotti session send demo --key Ctrl+P
tuiscotti session stop demo
tuiscotti machine
```

Choose one output/path convention and generate reference docs from the actual parser. Keep capture modes explicit: a piped CLI is not secretly a PTY. A deadline capture is incomplete, not a settled snapshot. Offline inspection/import never executes recorded commands.

Define a stable outcome taxonomy and explicit child-status policy. Default tool success must not hide an export/assertion failure merely because the child exited zero. A passthrough/run mode may preserve child exit status, but its contract must distinguish infrastructure failures and record all fields in structured output. Do not overload one undocumented number with unrelated meanings.

Mandatory parser tests: `--machine` before/after `--`, empty/native/non-UTF-8 arguments where supported, repeated formats, invalid format, path beginning with a dash, negative numeric arguments, spaces/newlines, mutually exclusive flags and missing command. Help, docs, completion output and actual parser must agree.

## 6. Goal G7 — Enforced workspace, tooling and compilation design

The exact requested lint/config tables are supplied in `config-reference/`. They are policy fragments; no claim is made that the current code passes them. Apply them, fix the code, validate selected tool schemas and prove enforcement with negative tests.

### 6.1 Workspace graph

```text
tuiscotti-core <--- tuiscotti-render
      ^       <--- tuiscotti-runtime ---> termpane [process/pty]
      ^       <--- tuiscotti-insta ------> insta (+render only when needed)
      ^                  ^
      +-------------- tuiscotti facade
                              ^
                         tuiscotti-cli ---> clap

xtask: development-only; does not depend on product libraries
fixtures: nonpublished real applications; consumer tests depend on the facade
```

All package metadata/lints/dependencies are inherited through the virtual workspace. The facade is a deliberate re-export layer, not a dumping ground. Public names stay short despite the internal graph. Split only when it isolates expensive dependencies, distinct ownership or measured rebuilds; do not create a crate per file.

Recommended default facade features: Ratatui + Insta + rendering for the normal screenshot workflow, **without** runtime/PTY. Provide a lighter structured-view configuration without rendering. The runtime and CLI dependencies must never leak into that profile. Verify every declared feature combination outside workspace feature unification.

Renderer edits should not rebuild terminal transport; CLI/xtask edits should not rebuild the data model; pure view tests should not compile PTY code. Benchmark these change scenarios, clean builds, warm mbx builds and cold native build cost. Add no package solely to make a dependency graph diagram more impressive.

### 6.2 Exact strictness and limits

Use resolver `3`, edition `2024`, `rust-version = "1.98"`, and intended first-party license `MIT OR Apache-2.0`. Every member has `[lints] workspace = true` and workspace inheritance for package metadata. Keep the full user-specified Rust/Clippy/rustdoc policy, including forbid unsafe and denied ignored results, stale expectations, unwrap/expect/panic in ordinary code, TODO/debug/memory-forget, lock/future hazards and undocumented suppression.

Use Rust **1.98.1** for normal development and release CI; the Rust team identifies it as the patch fixing a 1.98.0 vtable miscompilation. Preserve the user's `1.98` API floor while separately verifying compatibility and recording this patch requirement; do not ship release artifacts built with the defective patch merely to demonstrate MSRV. [R20]

`clippy.toml` permits expect/panic only where Clippy recognizes test code; it does not justify runtime panics or unwraps in examples. Keep tests in separate files. Prefer `Result` and `?` in examples/fixtures. Assertions intentionally fail tests; preserve native assertion semantics and use only tightly scoped justified lint expectations when genuinely necessary. Never relax `unsafe_code = "forbid"` or move unsafe implementation into another product-owned crate to evade it.

Limits: 80 Clippy lines per function, 400 physical lines per ordinary Rust file, 150 per src/lib.rs and src/main.rs. The adapted alint globs cover all crate names, the facade and xtask—not the obsolete `crates/tui-snap-*` prefix. Add validator self-tests for an 81-counted-line function, 401-line file, 151-line root, missing required file, wrong source location, forbidden script and missing workspace inheritance. Ensure rules match real files after renaming. [R21,R22]

### 6.3 Dependency source and licensing policy

Keep the supplied cargo-deny configuration exactly unless a real unsupported schema key is proven and a semantically equivalent supported spelling is required. Do not broaden license/source allowlists, ignore advisories, or enable Git globally to unblock a build. Track all actual normal/build/dev/target dependencies centrally.

`unknown-git = "deny"` plus an empty allowlist means termpane delivery must use a registry release. Crate-source audit must inspect renamed dependencies and target-specific declarations, not just grep three strings. Internal workspace path+version edges are normal; a patched external checkout is not. [R23]

`MIT OR Apache-2.0` describes only first-party source for which the necessary rights are established. Audit provenance before relicensing: repository ownership alone does not establish rights to relicense every copied file. Preserve third-party and font notices; font licenses are a separate asset inventory and must not be overwritten by workspace metadata. Any unresolved rights issue is explicitly recorded and blocks a false dual-license claim. Cargo license allowlists do not relicense assets. [R24]

### 6.4 Mise and Mr. Boxington

Latest stable mbx observed in this review: **1.20.0**, published September 28, 2026. Recheck at execution and pin the verified stable version in Mise. The official project documents a Mise installation and wrapping Cargo commands with mbx. Use the actual selected version's CLI, not remembered flags. [R25,R26]

Mise owns reproducible tools: Rust toolchain coordination, mbx, nextest, cargo-insta, cargo-deny, alint and any necessary Rust task tools. Avoid conflicting duplicate Rust pins; either derive one configuration from the authoritative pin or check agreement mechanically.

Every compiling repository task must use mbx, including xtask startup, tests, Clippy, docs, examples and packaging. Prefer a documented native command such as `mbx run --package xtask -- verify`, with xtask invoking verified `mbx check`, `mbx clippy`, `mbx nextest run`, and `mbx test --doc` forms. Read-only metadata tools need not pretend to compile. Do not recursively stack cache wrappers.

For parallel builds use distinct target directories where needed; share mbx's cache/resource policy, not a mutable Cargo target lock. Measure cold/no-cache and warm-cache behavior. Keep untrusted PR jobs from publishing trusted remote caches; qualify inputs and artifacts. Do not assume every native link is cacheable or that a hit rate proves faster complete CI. [R26]

### 6.5 Renovate

Use checked JSON/JSON5 configuration and the existing external Renovate service or generated scheduled job. No repository Node project. Validate with the official validator provided by a pinned external distribution/container.

Use supported Cargo and Mise managers. Cover workspace manifests, lockfiles, mbx and other tool pins. Keep MSRV policy separate from development-toolchain updates. Do not assume resolver 3 or Renovate itself proves every updated dependency remains MSRV-compatible: test the graph. [R27–R29]

Separate toolchain, termpane, rendering/font/encoding and pre-1.0 API updates from ordinary low-risk maintenance. Do not group unrelated risky upgrades or auto-accept snapshots in dependency PRs. Keep the dependency dashboard, explicit concurrency/rate controls and regular lockfile maintenance; security updates must not be indefinitely delayed behind routine batches. Begin without automerge until the required gates and evidence are proven. Verify any chosen release-age settings and data-source support rather than blindly copying generic presets.

Generated `.github` files remain Velnor-owned. Configure Renovate against generator inputs or upstream pins, and regenerate. It must not independently patch generated YAML that a later generator run overwrites. A missing generic generator capability belongs in a reviewed Velnor change, not a local scripting escape hatch.

## 7. Goal G8 — One new name and a complete branding migration

### 7.1 Recommendation

**Tuiscotti** is a coined TUI + biscotti-style name: memorable, Rust-friendly and in the same playful naming spirit as Ratatui without copying it. Proposed tagline: **“Crisp snapshots. Real terminal tests.”** The technical descriptor must remain explicit: **“Rust TUI visual-regression toolkit.”** This is a naming proposal, not a claim of legal or worldwide uniqueness.

An exact GitHub repository-name search returned no match during this review. A crates.io API check was not accessible and registry availability was not conclusively established. Before any irreversible change, check the exact crate/binary/repository names, normalized hyphen/underscore conflicts and obvious competing software use. Do not reserve/publish packages merely as a name probe. If a genuine conflict is found, choose and record another comparably distinctive name autonomously before changing source; no mixed names.

### 7.2 Canonical identity map

| Surface | Target |
|---|---|
| Display name | Tuiscotti |
| Facade crate / Rust import | `tuiscotti` |
| CLI executable | `tuiscotti` |
| Internal crates | `tuiscotti-core`, `tuiscotti-render`, `tuiscotti-runtime`, `tuiscotti-insta`, `tuiscotti-cli` |
| Test fixture package | `tuiscotti-fixtures` (not published) |
| Configuration | `tuiscotti.toml` |
| Environment variables | `TUISCOTTI_*` |
| Candidate artifact namespace | `tuiscotti` |
| GitHub repository | `tailrocks/tuiscotti`, using rename of the existing repository when permission is available |
| Docs/packages/caches/installer metadata | Same identity, with schema/tool versions separate from marketing names |

Termpane retains its own name and repository. Do not rename upstream dependencies.

Rename the existing repository, not create a replacement that loses PR/issues/history. Update remotes, description/topics, docs links, generated CI inputs, release/package metadata, CLI schemas/help/completions, test fixtures, error text, new artifact tags and environment variables. Verify repository identity and redirected historical links. Existing published package versions cannot be rewritten; use an explicit migration statement rather than claiming they disappeared.

Do not change immutable historical approval bytes to remove old provenance. Keep narrowly scoped documented exceptions for frozen artifacts, provenance, source citations and migration history. No active old-name binary aliases, duplicate configurations or compatibility branches. A branding validator must reject old active identifiers without flagging preserved historical evidence as something to overwrite.

## 8. Implementation order, independent proof and exit criteria

| Phase | Work | Exit proof |
|---|---|---|
| P0 | Refresh PR/reviews; preserve dirty work and approval hashes; record eight-goal supersession and name screening | Evidence/ownership inventory; no rollback to a stale PR head. |
| P1 | Add independent regressions for argv boundary, caller metadata, sample identity, unsupported capabilities and applicable old verification checks | Tests fail on wrong behavior for the right reason. |
| P2 | Implement termpane upstream process/PTY and observation gaps; qualify and release | Registry-consumer test; no patched source or relaxed deny policy. |
| P3 | Virtual workspace, strict lints, crates-only, xtask; remove foreign clients/scripts | Negative policy tests pass and no runtime unsafe exceptions. |
| P4 | Wire termpane; simplify public facade and CLI; complete conversion of call sites | No direct banned dependency under any feature/target; normal/advanced API examples compile. |
| P5 | Rust fixture apps and five requested/retained formats; Insta and nextest stress/remap coverage | Real view/interaction/pipe tests; unchanged historical approvals; wrong artifacts fail. |
| P6 | Canonical docs, detailed syntax comparisons, measured compile graph, mbx/Renovate/generator | Docs and schema checks; reproducible cold/warm numbers; required CI evidence. |
| P7 | Finish one-name migration, final review, packaging/consumer installation | No active old brand or clients; clean published-dependency consumer; all eight gates evidenced. |

Real concurrent work is encouraged with nonoverlapping ownership, a single integration owner per Git worktree, independent reviewers and bounded build parallelism. One PR branch in each affected repository is preferable to many branches. Dependency sequencing must not be bypassed because a downstream integration is blocked on upstream correctness.

### Completion gate

G1: all terminal functionality uses the qualified released termpane interface; no direct portable-pty/alacritty_terminal/libc or local substitute.

G2/G3: Rust facade and CLI only; no Python/TS/JS clients or first-party executable scripts (including embedded JS). Required maintenance functionality survives in xtask.

G4: one authoritative documentation tree with honest revision-pinned comparisons and no stale completion claims.

G5: real Rust fixture apps, view/controller separation and explicit ANSI/TXT/ASCII/PNG/HTML format contracts with negative tests.

G6: deliberate facade, typed errors, std-like builders/paths, bound locators, correct Insta call-site metadata, native CLI parser, advanced options and documented outcomes.

G7: every member inherits the requested workspace/lints; strict tools and policy mutations execute; Rust/mbx/Mise pins are proven; Renovate and generated CI agree; crate boundaries show measured benefit or are simplified.

G8: one screened brand throughout the active product, packaging and remote identity; historical immutable evidence is preserved rather than rewritten.

Also retain the previous product guarantees not superseded: three testing modes, exact canonical/pixel checks, consistency across Rust/CLI/report, immutable references, safe Rust-owned protocol/recording interfaces, isolated nextest run/attempt identities, final-output/cancellation evidence, honest supported-platform reporting and independent adversarial tests. Missing external permission, registry release or licensing clearance is a real incomplete gate, not a reason to fabricate completion.

## Sources

The URLs identify evidence read for this specification; linked source files are the authority, not earlier prose summaries.

- **R1:** PR metadata/discussion: https://github.com/tailrocks/tui-snap/pull/6 (inspected head `7e8272bc08dd3241d731f832c573fd4c6de3fe1e`).
- **R2:** PR manifest: https://github.com/tailrocks/tui-snap/blob/7e8272bc08dd3241d731f832c573fd4c6de3fe1e/Cargo.toml
- **R3:** Runtime capabilities: https://github.com/tailrocks/tui-snap/blob/7e8272bc08dd3241d731f832c573fd4c6de3fe1e/src/tui.rs
- **R4:** Termpane scope: https://github.com/tailrocks/termpane/blob/8ff87fe1795b5a246214e9dc8a2a000c8746dab5/README.md
- **R5:** Termpane manifest: https://github.com/tailrocks/termpane/blob/8ff87fe1795b5a246214e9dc8a2a000c8746dab5/Cargo.toml
- **R6:** Termpane public exports: https://github.com/tailrocks/termpane/blob/8ff87fe1795b5a246214e9dc8a2a000c8746dab5/src/lib.rs
- **R7:** Current README: https://github.com/tailrocks/tui-snap/blob/7e8272bc08dd3241d731f832c573fd4c6de3fe1e/README.md
- **R8:** Public exports: https://github.com/tailrocks/tui-snap/blob/7e8272bc08dd3241d731f832c573fd4c6de3fe1e/src/lib.rs
- **R9:** Locator example: https://github.com/tailrocks/tui-snap/blob/7e8272bc08dd3241d731f832c573fd4c6de3fe1e/examples/05-locators-waits.rs
- **R10:** CLI parser: https://github.com/tailrocks/tui-snap/blob/7e8272bc08dd3241d731f832c573fd4c6de3fe1e/src/main.rs
- **R11:** Assertion integration: https://github.com/tailrocks/tui-snap/blob/7e8272bc08dd3241d731f832c573fd4c6de3fe1e/src/assert.rs
- **R12:** Command/binary resolver: https://github.com/tailrocks/tui-snap/blob/7e8272bc08dd3241d731f832c573fd4c6de3fe1e/src/command.rs
- **R13:** tui-test public bound locators: https://github.com/microsoft/tui-test/blob/main/crates/tui-test/src/runtime.rs (retrieved 2026-09-29, blob `e6aca5792cfdea81c46c4977a365e68f33099695`).
- **R14:** Terminal Control Rust API: https://github.com/anomalyco/terminal-control/blob/main/docs/rust-library.md (retrieved 2026-09-29, blob `6e5c17c5c79f89229f88c0cba677de31711256e2`).
- **R15:** termlens Rust workflows: https://github.com/vyncint/termlens/blob/main/README.md (retrieved 2026-09-29, blob `e653ffcd0bec0dcc20fb8b5e30b4c366bf9ad1ac`).
- **R16:** Nextest runtime metadata: https://nexte.st/docs/configuration/env-vars/
- **R17:** Rust API Guidelines: https://rust-lang.github.io/api-guidelines/checklist.html
- **R18:** Builder guidance: https://rust-lang.github.io/api-guidelines/type-safety.html
- **R19:** Microsoft library resilience guidance (one primary reference, not universal law): https://microsoft.github.io/rust-guidelines/guidelines/libs/resilience/
- **R20:** Rust 1.98.1 release: https://blog.rust-lang.org/2026/09/03/Rust-1.98.1/ (retrieved 2026-09-29, identifies 1.98.1 of September 3).
- **R21:** Workspace/resolver: https://doc.rust-lang.org/cargo/reference/workspaces.html and https://doc.rust-lang.org/cargo/reference/resolver.html
- **R22:** Clippy and alint: https://rust-lang.github.io/rust-clippy/master/index.html and https://alint.org/docs/rules/
- **R23:** cargo-deny sources: https://embarkstudios.github.io/cargo-deny/checks/sources/cfg.html
- **R24:** cargo-deny licenses: https://embarkstudios.github.io/cargo-deny/checks/licenses/cfg.html
- **R25:** Mr. Boxington release: https://github.com/jdx/mr-boxington/releases/tag/v1.20.0
- **R26:** Mr. Boxington official usage: https://github.com/jdx/mr-boxington and https://github.com/jdx/mr-boxington-action
- **R27:** Renovate Cargo manager: https://docs.renovatebot.com/modules/manager/cargo/
- **R28:** Renovate Mise manager: https://docs.renovatebot.com/modules/manager/mise/
- **R29:** Renovate configuration: https://docs.renovatebot.com/configuration-options/
- **R30:** Insta configuration/review: https://insta.rs/docs/advanced/

Configuration snippets came from the user's requested policy, adapted only for actual renamed paths and coverage. They have not been run through Clippy, cargo-deny or alint in this environment; implementation must validate them and the resulting project.
