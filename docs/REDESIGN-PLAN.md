# tui-snap redesign plan

Status: proposed architecture; not implemented or benchmarked.
Research date: 2026-09-28.
Research baseline: retrieved default-branch heads listed below. The source files and implementations are authoritative; earlier comparison documents remain historical records.

## Product decision

Keep tui-snap and redesign it as a Rust-first terminal testing platform. Do not reduce it to a snapshot helper or make it a wrapper around tui-test or Terminal Control.

Product definition:

> tui-snap tests terminal applications at three levels: ordinary CLI processes, real interactive terminals, and pure Ratatui views. It uses one observation and comparison model, integrates natively with Insta, and runs naturally under cargo-nextest.

Unify the useful strengths of:

- tui-snap: direct Ratatui rendering, complete grid concepts, visual baselines, explicit font resources, offline rendering, missing-glyph diagnostics, and ANSI/TXT/PNG/HTML evidence.
- tui-test: locators, retryable observational assertions, input operations, terminal-state inspection, structured diagnostics, and one engine behind multiple interfaces.
- Terminal Control: owned/named sessions, explicit capture reasons, retained final screens, recording/replay, machine interfaces, and optional application semantics.

Keep evidence and useful capabilities, not the current API structure or architectural mistakes. Breaking changes are acceptable. No legacy shims or deprecation paths.

### Capability comparison

| Area | tui-snap today | tui-test | Terminal Control | Plan |
|---|---|---|---|---|
| Pure Ratatui view tests | Direct production rendering and canonical frames | No equivalent first-class fixture workflow | Public frames allow an adapter, but no packaged workflow | Make this the fastest entry point |
| Interactive applications | Basic keyboard, mouse, resize, and waits | Rich locators, assertions, input, and shell integration | Owned/named sessions, interaction, retained state | Add a comprehensive Rust Tui API |
| Ordinary CLI contracts | Primarily terminal/snapshot-oriented | Primarily shell/PTY-oriented | Primarily session-oriented | Add a separate piped-process API |
| Canonical state | Strong grid representation | Rich observations, snapshots serialize a subset | Public frames, but extraction loses source distinctions | Preserve complete state with explicit policies |
| Screenshots | Pinned-font renderer and visual artifacts | PNG/SVG and recordings | SVG/PNG and recordings | Keep deterministic rendering separate from capture |
| Snapshot approval | Explicit, with comparison and storage correctness gaps | Missing snapshots are written even when update is false | Capture artifacts and test-client integration | Use native Insta plus explicit frozen references |
| Failure diagnosis | Expected/actual/diff reports | Structured failures and trace viewer | Session artifacts and recorded timelines | Combine visual difference with the action that caused it |
| Agent workflows | Small CLI surface | CLI, schemas, skills, monitor, language APIs | Named sessions, MCP, live interaction | Optional interfaces over the same core |
| Runner integration | No first-class Insta/nextest contract | Not centered on this workflow | Not centered on this workflow | Treat both as foundations |

Do not copy the competitors' correctness gaps. The current tui-test implementation writes a missing snapshot even when update is false, and exposing state is not proof that an assertion checks it. Terminal Control's extraction currently resolves colors to RGB, applies inverse by swapping colors, skips continuation/spacer cells, and collapses underline styles. Those may be valid presentation choices, but not a source-state verification contract. tui-snap schema v3 already preserves hidden and blink flags; do not report them as missing.

## Goals and boundaries

1. Make the production Ratatui view function easy to test with fixture data and explicit view state, without booting business logic, services, a process, or a PTY.
2. Test ordinary CLI contracts through pipes, preserving stdout and stderr separately, exit status, stdin behavior, and raw bytes.
3. Test real interactive applications through owned PTY sessions and a qualified terminal backend.
4. Provide the same naming, error reporting, artifact policy, snapshot configuration, and runner context across all three modes.
5. Keep one validated observation model and one comparison engine across Rust assertions, CLI checks, offline checks, and reports.
6. Make visual artifacts reproducible and useful for human review, while distinguishing deterministic terminal-like rendering from exact correspondence with a particular desktop terminal.
7. Make Insta review and nextest execution foundational integrations, not optional afterthoughts.
8. Keep pure view consumers independent of child processes, PTY support, native terminal engines, CLI tools, network, and daemon setup.
9. Reuse upstream terminal, PTY, font, shaping, and encoding libraries. Do not build a new VT emulator or embed a competing framework.
10. Preserve frozen approvals during implementation. Renderer/profile changes use separate versions and separately reviewed evidence.

This remains a research project. Do not describe it as production-ready or claim competitor parity, backend qualification, or performance results before the required proof exists.

## Three testing modes

### Pure Ratatui view tests

Input: fixture model, explicit view state, viewport, and theme.

Execution: the actual production Ratatui rendering function, using a buffer, completed TestBackend frame, draw closure, or stateful widget.

Output: validated screen, targeted state assertions, and optional screenshot assertions.

This is the fastest and simplest entry point. Components do not need an artificial testing trait. No application startup, controller, database, network, PTY, or event loop is required.

### Ordinary piped CLI process tests

Input: executable, arguments, stdin, environment, and working directory.

Output: separate stdout/stderr bytes plus exit, signal, timeout, output-limit, and diagnostic timing information.

Support stdin EOF, non-UTF-8, bounded output, deadlock-safe concurrent pipe draining, signals, and timeouts. Do not invent a precise global ordering between independently captured stdout and stderr. Resolve already-built binaries; never invoke Cargo from every test. A PTY is not the implementation of this mode because non-terminal stdout behavior is part of the contract.

### Interactive terminal tests

Input: real executable, terminal profile, and real keyboard, mouse, resize, focus, and other supported terminal events.

Output: atomic terminal observations, interaction trace, assertions, and artifacts.

The session owns cleanup on success, error, panic, and cancellation. Advanced callers may explicitly finish to receive teardown errors. This mode validates controllers and terminal behavior and complements pure view tests; it does not replace them.

## Correctness work before feature work

Fix the verification architecture before adding surface area:

- Remove any path that infers pixel equality from cell equality. Same cells and dimensions with different decoded pixels must fail a strict image check.
- Remove approved-image-to-actual copying. Candidate evidence must come from the candidate render; a skipped check is explicitly not-checked.
- Use one comparison implementation and persist its structured verdict. Test assertions, grouped checks, offline CLI checks, and reports must not invent different equality rules.
- Strict image equality means equal dimensions and equal decoded pixels under an explicit alpha policy. Perceptual similarity is diagnostic, never proof of strict equality.
- Missing or corrupt required references fail in frozen mode. Candidate output must never regenerate a frozen expectation.
- Publish canonical state, PNG, render profile, and evidence as one logical generation. Interrupted or partly approved sets cannot become a passing mixed generation.
- Make assertion outcomes difficult to ignore. A successful capture or completed check is not automatically a successful comparison.
- Every check must distinguish matched, mismatched, missing-reference, invalid-reference, unsupported, capture-incomplete, and not-checked. Not-checked must never mean matched.

Current high-priority examples to qualify: cell-equality shortcut in PNG comparison; grouped store reusing approved PNG/HTML as candidate evidence; grouped report checking PNG bytes while test checks decoded images and thresholds; per-file approval permitting mixed generations; assertion API requiring a separate outcome-enforcement step.

## Core model

Use explicit concepts with distinct responsibilities:

- **ProcessOutput**: separate stdout/stderr bytes, termination classification, output bounds/truncation, and diagnostic timing.
- **Screen**: immutable validated grid with dimensions and origin; full row-major source graphemes; explicit widths and continuations; source color identity and resolved appearance state as needed; independent modifiers; styles, links, and cursor intent.
- **Observation**: one atomic capture containing a screen and terminal modes, palette, title, clipboard, bells, scrollback, graphics observations, optional semantics, revision, capture reason, capability coverage, and diagnostics. Do not combine cells from one instant with cursor or palette from another.
- **TerminalProfile**: advertised/emulated capabilities, keyboard and mouse protocols, color behavior, and platform limits. Never advertise what the selected backend cannot implement.
- **RenderProfile**: exact font bytes and hashes, style faces, fallback order, geometry, scale, palette resolution, cursor policy, sampled blink phase, and renderer behavior/version.
- **ComparisonPolicy**: explicit contract for text, styled cells, terminal state, rendered pixels, and representation bytes. Exclude informational timestamps, PIDs, and cumulative counters by default. Cursor shape or palette changes must not disappear merely because text is unchanged.
- **ArtifactSet**: candidate generation, provenance, checks performed, hashes, output bounds, and a completion manifest. It is complete only after the manifest is published.

Unknown and unsupported are distinct from false, empty, or default. A required unavailable property produces unsupported. Keep source state, displayed content, and safe exported evidence separate. Terminal concealment is not redaction.

## Public Rust experience

Expose one normal facade. Examples here are proposed API shapes, not current features.

### View test

~~~rust
#[test]
fn settings_view() -> tuisnap::Result<()> {
    let model = fixtures::settings();
    let screen = tuisnap::ratatui::render((100, 30), |frame| {
        myapp::ui::render_settings(frame, &model);
    })?;
    tuisnap::assert_screenshot!("settings", &screen);
    Ok(())
}
~~~

The common screenshot call captures canonical state and rendered pixels as one sample, creates failure evidence before asserting, and verifies both. A cheaper structural snapshot still checks styles, geometry, and documented cursor policy; it must not silently degrade to text-only comparison.

### Piped process test

~~~rust
#[test]
fn rejects_invalid_config() -> tuisnap::Result<()> {
    let output = tuisnap::Command::cargo_bin("myapp")?
        .args(["--config", "invalid.toml"])
        .current_dir(fixtures::directory())
        .output()?;
    output.expect().code(2)?;
    tuisnap::assert_cli_snapshot!("invalid-config", &output);
    Ok(())
}
~~~

Interoperate with std::process::Command and existing CLI helpers. Reuse compatible Insta command-output conventions instead of creating gratuitously different ones.

### Interactive test

~~~rust
#[test]
fn opens_settings() -> tuisnap::Result<()> {
    let mut app = tuisnap::Tui::cargo_bin("myapp")?
        .size(100, 30)
        .spawn()?;
    app.get_by_text("Ready").expect_visible()?;
    app.press("Ctrl+P")?;
    app.get_by_text("Settings").click()?;
    let screen = app.snapshot()?;
    tuisnap::assert_screenshot!("settings-open", &screen);
    app.press("q")?;
    app.expect_exit().success()?;
    Ok(())
}
~~~

Offer typed keys as well as parsed chords, explicit capture policies, custom predicates, region comparisons, renderer injection, and raw bytes. Keep common tests free from manual provenance construction, renderer instances, storage roots, transport messages, and timeout structures. Defaults belong in a small tui-snap.toml with test-level builder overrides.

## Query, wait, and input contracts

Locators should query current screen revisions and compose text, regex, style, link, region, and optional semantic matches. Support scoped/relative queries, occurrence selection, intersections, unions, and containment.

- Never cache coordinates across repaints.
- Actions require a unique target unless the caller explicitly disambiguates.
- Multiple matches fail clearly.
- Scrollback is logical history and cannot be clicked as visible viewport content.
- Never infer disabled or focused semantics from appearance.
- Semantic roles, IDs, labels, focus, disabled state, and hit regions come only from an explicit provider. Semantics choose real keyboard/mouse input in E2E tests; they do not invoke application controllers directly.
- Retry observational assertions, not destructive actions. Locator readiness may retry; Submit or Delete executes once.
- Distinguish observe-now, wait-for-predicate, visual-stability wait, synchronized-frame wait, process-exit wait, and shell-command wait. A ready marker is not animation completion; output silence is not business completion.
- Distinguish absent now, eventually disappears, and remains absent for an interval.
- Support text, typed chords, raw bytes, key press/down/repeat/up, negotiated paste, hover/click/drag/wheel, modifiers, focus, resize, signals, clipboard, bells, modes, title, and palette changes. Unsupported operations fail explicitly.
- Optional deterministic event and clock helpers wrap caller-supplied update/render functions without imposing an application architecture.

## Insta and snapshot lifecycle

Do not create another general snapshot ecosystem. Use native Insta naming, pending changes, review, configuration, structured snapshots, source locations, names/suffixes, descriptions, and scoped settings.

- Use public Insta APIs, including its public custom comparator for decoded-pixel comparison. Its default binary comparison is byte-based and is not sufficient for pixel equality.
- Proposed assertions: CLI snapshot covers stdout, stderr, termination; styled screen snapshot covers canonical state; screenshot covers canonical state plus independently rendered pixels; direct expect APIs cover targeted behavior.
- Let consumers use insta JSON snapshots over a documented projection.
- Prototype the compound canonical-plus-image lifecycle. Candidate evidence is created before assertion and tied to the same capture. Partial acceptance cannot mix old and new generations. Do not copy private Insta internals; request a small upstream hook if public APIs cannot support the transaction cleanly.
- Support two explicit reference policies: evolving expectations reviewed through Insta and frozen references for historical visual-baseline workflows. Frozen stores reject missing/corrupt references and acceptance attempts.
- Disable auto-updates and force-pass in CI. Keep verification separate from Insta's force-pass collection workflow.
- Generate ANSI/TXT/PNG/HTML for every visual sample from the same generation. Canonical state and PNG are default expectations; ANSI/TXT/HTML remain review evidence unless byte comparison is explicitly requested. Existing frozen four-file trees stay importable and verifiable through a read-only path; the importer never changes the approved tree.
- Keep the familiar workflow: cargo nextest run, then cargo insta review.

## cargo-nextest integration

Keep ordinary #[test] as the default. Do not require a custom harness, attribute macro, or scheduler. Parameterized fixtures should produce separate identifiable cases when filtering and sharding matter; important scenarios cannot be hidden in one opaque test loop. Verify doc examples separately.

- Baseline identity: package, test binary, test, scenario, and explicit profile/variant.
- Scratch evidence identity: run, attempt, stress iteration, and shard. Do not use reused global/group slots as durable IDs.
- Prefer nextest remapped executable paths; support relocated archives and workspaces; never rely only on a build-machine absolute path or launch nested Cargo.
- Correlate supplementary artifacts with nextest JUnit and stable test identity. Reports never overwrite the runner result or invent success.
- Maintain a required-scenario manifest so full-suite runs detect skipped captures while filtered runs report partial coverage.
- Journal actions and observations incrementally. Missing completion marker means incomplete.
- Preserve failed-attempt evidence after retries; retries aid diagnosis and never silently convert a flaky test into trusted coverage.
- Own child cleanup and process reaping. RAII is insufficient for all hard-kill cases; qualify guardian or platform containment and document escaping descendants.
- Do not rely on nextest leak detection to catch PTY descendants.
- Isolate runtime directories, environment, ports, and candidate paths. No process-global CWD mutation, common mutable session, fixed port, or unscoped environment.
- Coordinate only resource-heavy test groups across nextest processes. In-process mutexes do not coordinate nextest's process-per-test execution.
- Measure CI fast-lane target under declared hardware/cache conditions; target is under 120 seconds, not a measured result.

An initial CI profile can use nextest's supported retry, leak, test-group, and JUnit settings:

~~~toml
[profile.ci]
fail-fast = false
retries = 0
flaky-result = "fail"
leak-timeout = { period = "500ms", result = "fail" }

[profile.ci.junit]
path = "junit.xml"

[test-groups]
terminal-e2e = { max-threads = 4 }

[[profile.ci.overrides]]
filter = 'test(e2e::)'
test-group = 'terminal-e2e'
~~~

Four concurrent terminal tests is an initial tuning choice, not a universal optimum. Use nextest's repetition facilities for stress qualification and retain each failure's artifacts.

## Runtime, dependency, and package strategy

tui-snap owns orchestration, queries, assertions, observations, and evidence. Upstream crates own PTY primitives, terminal emulation, font parsing/shaping, and encoding.

- Do not implement a VT emulator just to become a better testing framework.
- Do not embed tui-test, Terminal Control, or modified termlens source.
- Replace the current vendor/termlens and separate termpane arrangement through a qualified migration, not a renamed module.
- Qualify the official Ghostty binding as the first full-fidelity emulator candidate. It is a candidate, not an approved backend. Isolate native build requirements from pure Rust view consumers. No silent fallback.
- Optional Rust-native backends can support compatibility and build tradeoffs, but required unavailable state fails explicitly.

Suggested package boundaries:

~~~text
crates/
  tuisnap-core/       validated data, comparison policies, query evaluation
  tuisnap-render/     rendering, fonts, pixel comparison
  tuisnap-runtime/    pipes, PTY, lifecycle, shell/protocol integration
  tuisnap/            public API, Ratatui and Insta integration
  tuisnap-cli/        CLI and optional machine/session interfaces
~~~

These are internal isolation boundaries, not five user-facing APIs. Most consumers should depend only on tuisnap.

## Rendering, traces, CLI, and safety

### Rendering

Keep a reproducible initial profile while qualifying improvements separately. Strict profiles pin all font and fallback bytes, styled faces, geometry, scale, palette, cursor/blink policy, and renderer version. No system-font lookup or synthetic style substitution in strict approval mode. Source cell widths control layout so fallback does not shift the grid.

Qualify grapheme shaping, combining marks, CJK, Nerd icons, box drawing, Braille, blocks, clipped styles, and an explicit emoji policy. Missing glyphs fail strict visual approval by default; an explicitly allowed placeholder is reported. A deterministic placeholder is evidence of a missing glyph, not proof that the intended glyph rendered.

Blink intent remains in canonical state; a still image uses a declared deterministic phase. Caches are content-addressed by all relevant state, font/profile/renderer versions, and corruption is rejected. Qualification includes no-cache runs.

### Trace and evidence

A failure should show what action happened, what the app showed, which condition failed, and the exact state/pixels that differ. Record input, delivered bytes where enabled, observation revisions, waits, assertion outcomes, exits, and artifacts. Ship a readable summary, machine trace, and offline viewer/bundle. Diagnostic export failures remain secondary and cannot replace the original application/assertion failure.

### Security

Raw input/output recording is opt-in where it may contain secrets. Concealed text can remain in canonical state, so safe export must be explicit and tested. Terminal queries receive sandboxed clipboard behavior and controlled responses; escape sequences cannot read arbitrary files, access the host clipboard, or open arbitrary URLs.

### CLI

The CLI extends the library and covers:

| Job | Proposed surface |
|---|---|
| Project setup | init, doctor, config/schema inspection |
| Capture and interact | capture, optional session control, input, waits, state inspection |
| Saved evidence | render, diff, replay |
| Review | report, Insta-integrated visual review, controlled acceptance |
| Automation | JSON output, protocol schema, optional MCP |

Do not build another Rust test scheduler. A convenience test command, if included, delegates transparently to nextest and preserves its configuration and exit status. Use tui-snap.toml for tui-snap policy, .config/nextest.toml for execution, and standard Insta configuration for Insta behavior. Do not merge these into a proprietary universal config. The learning path starts with a short view test, not daemon setup.

## Implementation sequence

| Milestone | Deliverable | Required proof |
|---|---|---|
| M0 — Correct verification | Fix pixel shortcuts, report disagreement, missing references, and artifact consistency | Deliberately wrong pixels and missing artifacts fail |
| M1 — First-class view testing | New core model, production Ratatui adapters, ergonomic assertions | Pure view tests need no PTY/runtime dependencies |
| M2 — Insta + nextest | Native review, attempt-safe evidence, binary remapping, parallel safety | Consumer works filtered, sharded, retried, and relocated |
| M3 — CLI + terminal runtime | Piped processes, owned PTY sessions, qualified backend, cleanup, waits, input | Independent CLI/VT/lifecycle corpus passes |
| M4 — Interaction API | Locators, retryable assertions, optional semantics, deterministic event helpers | Wrong targets, duplicate matches, repeated actions detected |
| M5 — Visual diagnosis | Pinned rendering, complete artifacts, traces, offline review | Renderer-only changes fail and failures remain inspectable |
| M6 — Advanced parity | Named sessions, agent interfaces, recordings, optional clients/backends | Explicit competitor capability matrix passes |
| M7 — Consumer migration | Replace old APIs and remove obsolete code/dependencies | Required behavior and frozen references remain verified |

### First vertical slice

Implement one pure settings view, one piped CLI error case, and one real settings-navigation journey. All three run under nextest, produce appropriate Insta expectations, retain readable failure evidence, and clean up correctly. This is the first release target; do not lead with many CLI commands before the assertion lifecycle works.

### Qualification: test the tester

Use deliberate mutations for wrong glyph, style, continuation, cursor, one changed pixel, missing font, altered palette, deleted reference, partial approval, stale binary path, full stdout/stderr pipes, cancellation during wait, repeated destructive input, and retry evidence overwrite. Also cover concealed-data leaks, duplicate identity, late pipe output after process exit, parent hard kill, stale semantic revision, skipped scenario, and required scenarios not run.

A tool is not trustworthy merely because its tests pass; its verification machinery must reject each failure it claims to detect. Differential agreement between terminal engines is useful evidence, not the oracle.

Release rules:

1. P0 correctness mutations pass before storage/capture refactoring.
2. The pure-view + Insta + nextest slice works before broad agent features.
3. Piped and PTY paths pass independent lifecycle and terminal oracles.
4. Original frozen approvals remain unchanged during capture/model migration.
5. Renderer migrations get separately versioned profiles and separately reviewed evidence.
6. Claim competitor replacement only for shipped, qualified capabilities.
7. No unsafe cross-thread terminal sharing, third-party source forks, silent backend fallback, CI auto-accept, or successful status for absent captures.
8. Require macOS/Linux conformance; record Windows ConPTY differences and supported subset.
9. Measure cold/warm builds, startup, capture, render, memory, fixture throughput, and full-suite duration on declared hardware. The CI target is under 120 seconds; optimize and shard rather than removing fidelity checks.

## Source baseline

| Project | Inspected default-branch revision |
|---|---|
| tui-snap | 9dc86daff1dcbf20805b145916e8f04e9515f929 |
| tui-test | 7afb14b3c4075d24a7b9bf1a05175717f253821c |
| Terminal Control | c1d4f95e4f1b7638f6229e9bfc9599a955f95ce2 |
| Insta | 064742e9b7b2f3eaabb4724069e739ddf23d8227 |

Earlier repository research is historical and captured a different recommendation at earlier revisions. This plan supersedes it as the proposed direction after the newer source review; it does not erase or rewrite historical evidence. The research did not execute the proposed implementation or performance benchmarks.

The complete work-item checklist and source links are in [REDESIGN-BACKLOG.md](REDESIGN-BACKLOG.md).
