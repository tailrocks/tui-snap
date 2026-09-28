# tui-snap research and redesign assessment

Status: refreshed assessment. The former root RESEARCH.md is superseded by this file.

Research date: **2026-09-28**.

Current source baseline: 9dc86daff1dcbf20805b145916e8f04e9515f929
(origin/main, the latest source commit inspected). The redesign documents are
proposed design, not implementation: REDESIGN-PLAN.md and
REDESIGN-BACKLOG.md, added in commit
dad80a81a930907e76c5ed1c1f279529bf05165c.

This is a repository-source review. It did not run the test suite, execute
benchmarks, or qualify a competing backend. Claims below use these labels:

- **Implemented** means present in the inspected source or asserted by a checked-in test.
- **Proposed** means required by the redesign plan and not present in the current source.
- **Unverified** means the source or plan identifies work that still needs a proof run.

## Decision

The product still makes sense. Keep tui-snap, but finish the migration from a
single visual-regression crate into a Rust-first testing platform with three
honest entry points:

1. pure production Ratatui view tests;
2. ordinary piped CLI process tests; and
3. real interactive terminal tests.

Use one validated observation model and one comparison engine across those modes.
Integrate with Insta and cargo-nextest as first-class consumers. Reuse upstream
PTY, terminal-emulation, font, shaping, and encoding crates. Do not embed a
second test framework or maintain repaired third-party source copies.

No single inspected alternative combines the required pure-view fixture flow,
ordinary pipe semantics, owned PTY sessions, pinned glyph rendering, ANSI/TXT/
PNG/HTML evidence, snapshot review, and nextest-safe lifecycle handling. The
correct question is which responsibilities to reuse, not whether one competitor
can replace the whole product.

## User requirements and their boundaries

The requested workflow contains three related but different tests:

| Requirement | Correct test boundary | Current status |
|---|---|---|
| Verify a model and view without business logic | Production Ratatui draw function into a deterministic buffer, then canonical and optional image assertions | **Implemented** adapter; proposed ergonomic assertions |
| Verify a real application journey | Child process in a PTY, terminal input, waits, screen/state assertions, cleanup | **Implemented** fixture-oriented PTY session; proposed richer Tui/locator API |
| See what changed | Candidate and approved state rendered with a pinned profile, plus readable report/diff | **Implemented** renderer and stores; **unverified** full correctness contract |

A changed reference means review is required. It does not prove every application
state is wrong. An unchanged reference proves only the covered fixture or journey
still matches. Coverage of models, view states, terminal profiles, and journeys
remains a test-suite responsibility.

## What the inspected source actually provides

### Package and feature boundary

The crate is tuisnap version 0.2.0. Its default feature is pty; disabling
default features leaves the library's Ratatui/view, frame, renderer, snapshot,
and grouped modules without the optional PTY dependencies. The binary is gated
behind pty. The current manifest still uses a vendored termlens path and an
optional termpane git dependency. **No dependency migration has happened yet.**
[S1]

The public library exports Frame, cell/style types, Renderer, snapshot stores,
Ratatui adapters, and feature-gated PTY/raw-replay modules. It does not export
the proposed Command, Tui, locator, semantic-provider, Insta macro, or nextest
integration APIs. [S2]

### Pure Ratatui views — implemented foundation

ratatui::widget_frame renders a widget into a TestBackend and hides the backend
cursor for content-focused widget tests. ratatui::draw_frame renders a
caller-supplied draw closure, so production layouts and stateful widgets can be
tested without implementing a synthetic testing trait. ratatui::capture reads
the buffer and the backend cursor after drawing. The adapter maps Ratatui
colors/modifiers, wide symbols, continuation cells, and cursor visibility into
the canonical frame. [S3]

This is the strongest current differentiator. A pure view test does not start a
child, PTY, database, network, service, controller, or daemon. The current API
is lower-level than the proposed ten-line facade, but the underlying capture path
already matches the model → view requirement.

### Interactive PTY tests — implemented fixture path

pty::Session owns a termlens terminal, configures dimensions and terminal
environment, sends keys/text/paste/mouse input, resizes, waits, takes snapshots,
and exposes exit status. Drop delegates child cleanup to the owned terminal.
wait_for_text and custom wait_until return errors on timeout. wait_stable
compares full screen state through the backend, while wait_idle is an explicit
quiet-output fallback. run_once propagates readiness and action wait failures
and settles before returning a frame. [S4]

The current path is useful but narrow. It has no piped-process abstraction, no
fresh locator model, no semantic provider, no structured observation containing
terminal modes/palette/title/clipboard/bells/links/graphics, and no explicit
nextest attempt identity. Those are redesign work, not current guarantees.

### Raw ANSI replay — implemented with a narrower contract

The old hand-written SGR parser is gone. ansi::replay_raw feeds recorded raw
bytes through termpane::DamageGrid, with dimensions and bounded scrollback.
Cursor movement, alternate-screen behavior, scrolling, wide cells, and the
supported style flags are interpreted by the emulator. Normalized ANSI dumps
created from a Frame are explicitly debugging artifacts and must not be fed
back as raw state. [S5]

Raw replay preserves cursor position and visibility, but its canonical adapter
uses a block cursor and does not preserve cursor shape or blink phase. It also
does not expose the broader terminal protocol state proposed for Observation.
Use the PTY path when cursor shape or emulator behavior is part of the contract.

### Canonical frame — implemented schema v3

Frame is schema version 3 with strict validation and a maximum 512×512 import
dimension. It stores dimensions, row-major cells, provenance, and cursor state.
Each cell stores the source grapheme, width (0, 1, or 2), continuation flag,
default/indexed/RGB colors, and independent hidden, blink, bold, dim, italic,
underline, strikethrough, and reverse modifiers. Validation rejects malformed
dimensions/order, invalid widths, dangling continuations, invalid wide symbols,
and visible cursors outside the grid. [S6]

The current schema already preserves hidden and blink flags. Earlier reports that
listed those fields as absent are obsolete. The model intentionally does not
include all terminal protocol details, such as hyperlink targets, graphics
payloads, or sampled blink phase. The redesign must add an observation layer for
properties that are asserted, rather than silently pretending the current frame
is complete.

Frame::digest excludes provenance and includes cells plus cursor. text() is only
plain content; it is not a styled-screen assertion. diff_cells compares symbols,
geometry, colors, modifiers, and cursor changes. [S6]

### Renderer and artifacts — implemented, with explicit limits

The current renderer uses fontdue and pinned font bytes. It verifies the regular
face's measured geometry against the profile, rasterizes real glyphs at the
final scale, uses real styled faces and a pinned fallback chain, preserves
wide-cell geometry, draws underline/strikethrough, and freezes blink as visible.
Missing glyphs render as deterministic tofu and are recorded in a fidelity
sidecar. Fallback-served glyphs and face fallbacks are also recorded. [S7][S8]

The current default profile uses vendored JetBrains Mono Nerd Font faces, a
10×21 cell geometry, 2× raster scale, and pinned font/fallback hashes. This is a
reproducible terminal-like renderer. It is **not** proof of pixel identity with
Ghostty, Terminal Control, a user's installed terminal, or any other desktop
terminal. That distinction is part of the redesign's RenderProfile contract.
[S8]

Renderer::render_artifacts generates ANSI, TXT, HTML, PNG, and fidelity data
from one render pass. HTML uses the PNG as the authoritative visual and adds a
selectable SVG overlay plus embedded frame JSON whose informational timestamp is
normalized. The CLI supports offline JSON-frame rendering to txt, ansi, json,
svg, html, and png. [S7][S9]

The source tests assert real glyph differences (A versus B), deterministic
reruns, CJK two-cell geometry, fallback diagnostics, and format output. Those
assertions correct the former report's claim that PNG output consists of
rectangular placeholders. They were not executed during this documentation
refresh. [S10]

The current import/export boundary is still asymmetric: JSON frames can be
validated and rendered; HTML embeds JSON for lossless manual extraction; there
is no general CLI command that imports PNG or HTML into a canonical frame, and
PNG pixels cannot reconstruct source cell state. Importing a frozen four-file
tree is a proposed read-only migration feature, not current behavior.

### Snapshot stores — useful evidence, incomplete verification contract

The classic snapshot::Store writes actual frame JSON, PNG, and fidelity data
before comparison. It keeps approved files untouched during checks, writes
visual diffs, fails closed on a missing approved frame, rejects corrupt frames,
and requires explicit accept. These are implemented and valuable for failure
diagnosis. [S11]

The grouped store supports nested names and four approved artifacts per scenario:
ANSI, TXT, PNG, and HTML. Actual artifacts include a frame sidecar and fidelity
sidecar; reports link scratch files. Missing grouped artifacts fail closed, and
there is no environment-variable auto-bless path. [S12]

The following correctness gaps remain in the inspected source and are P0 work:

1. diff::compare_png_with_flags treats different decoded PNGs with equal
   dimensions as score 1.0 when ansi_matched is true. This lets cell equality
   stand in for independent pixel equality. [S13]
2. The grouped fast path writes approved PNG/HTML bytes as the actual candidate
   when ANSI and TXT match. The resulting files are not proof that the current
   renderer produced those bytes. full_render exists as an opt-in escape hatch,
   but the default verification path remains misleading. [S12]
3. GroupedStore::report_with derives status from on-disk byte equality. It does
   not use the decoded-pixel threshold or rerun the same comparison engine as
   check_with_options; a report and a test can disagree. [S12]
4. Classic missing approved PNGs can be regenerated in memory from the approved
   frame. In the same-cell case the status can then become matched, even though
   the approved image is absent. Frozen visual mode must reject missing image
   references. [S11]
5. Acceptance is atomic per file, not per logical sample. An interruption while
   accepting frame/PNG or the four grouped artifacts can leave a mixed
   generation. [S11][S12]
6. The status enum does not represent the redesign's full result vocabulary
   (unsupported, capture-incomplete, not-checked, and related states). Reports
   and callers therefore cannot yet distinguish every incomplete or unavailable
   check from a match. [S11]

These are architectural correctness issues, not reasons to discard the product.
The redesign backlog makes them release-gating mutations before new runtime
features. [S15]

### Current CLI — implemented surface, not the proposed facade

The current binary provides render, check, accept, report, and run. run launches
a PTY command and can send scripted keys, waits, and formats; check and report
operate on the existing classic or grouped stores. There is no ordinary piped
Command API, no implicit Cargo build, no tuisnap test scheduler, and no
session/agent protocol. [S9]

## Corrected assessment of the old report

The former root report was useful research, but several statements described an
older implementation. They are replaced here rather than carried as competing
recommendations:

| Former claim | Current evidence | Correct interpretation |
|---|---|---|
| PNG draws placeholder blocks instead of glyphs | Renderer uses fontdue, pinned faces, fallback, and fidelity records; tests assert A ≠ B pixels | Obsolete. Current PNGs render real glyphs, with explicit tofu/fallback diagnostics |
| PNG/SVG use the old 9×20/9×18 geometry | Profile pins 10×21 cells and 2× scale; HTML and PNG share the renderer profile | Obsolete numbers. Renderer/profile identity still needs to be treated as a contract |
| A handwritten ANSI parser drops cursor motion | ansi::replay_raw uses termpane::DamageGrid; normalized dumps are not replay input | Obsolete for raw replay; protocol coverage still remains narrower than Observation |
| Ratatui capture loses cursor and width state | ratatui captures backend cursor and emits width/continuation cells; Frame validates both | Obsolete for the main adapter; raw replay still has cursor-shape limits |
| PTY waits can be ignored by run_once | run_once propagates wait_for_text, wait_idle, and wait_stable errors | Fixed in current source; richer distinct wait contracts remain proposed |
| Baselines are hash-only | Classic approved stores retain frame JSON and PNG; grouped stores retain four artifacts | Obsolete for current storage; grouped approved trees intentionally omit frame sidecars |
| Import/export is already a complete reusable round trip | JSON frame import/render works; PNG/HTML do not reconstruct a frame through a general import command | Still incomplete; frozen-tree importer is proposed |
| tui-snap should shrink to Ratatui + Insta glue | Current source has useful capture, renderer, and evidence work, while the redesign assigns clear ownership boundaries | Superseded. Keep the differentiators and replace correctness gaps/API architecture |

## Replacement landscape

The alternatives are complementary components:

| Project or component | Reuse decision |
|---|---|
| Ratatui TestBackend | Keep as the direct-view execution substrate; current adapter already uses it |
| Insta | Make native snapshot naming/review/configuration the default integration; do not create a second snapshot ecosystem |
| termlens | Current vendored PTY engine proves the Rust-test path; qualify an upstream dependency or replacement during runtime migration |
| Terminal Control | Qualify as a full-fidelity backend/session/reference candidate; its current extraction and renderer must not be assumed lossless or pixel-identical |
| tui-test | Borrow locators, retryable observation assertions, input operations, and structured diagnostics; do not copy its snapshot semantics |
| image-review tools | Reuse only if they can consume the pinned output contract and preserve the original test result; they cannot replace capture or terminal emulation |

The pinned competitor findings used by the redesign are:

- tui-test demonstrates a broad interaction/query surface, but its snapshot
  serializer is not the required complete source-state contract. [S16]
- Terminal Control demonstrates owned sessions, recording, machine/agent
  interfaces, and an optional semantic protocol. Its extractor resolves or
  drops distinctions that exact source-state verification needs, and its SVG/
  raster path does not prove desktop-terminal pixel identity. [S17][S18][S19]
- Insta provides native snapshot review and a public custom comparator, but
  binary snapshots compare bytes by default and ordinary macros do not create a
  multi-artifact transaction for a canonical frame plus image. [S20][S21][S22]

These are source/documentation comparisons at the pinned revisions. No benchmark
or full-corpus differential run was performed. A competitor capability is not
claimed as shipped tui-snap functionality until an acceptance item passes.

## Proposed target architecture

The target is defined in the redesign plan; the short form is:

~~~text
pure Ratatui view ─┐
piped CLI process ──┼─> validated Observation/Screen -> one comparison engine -> Insta/evidence
interactive PTY ───┘                                  -> nextest-correlated artifacts
~~~

Use these internal package boundaries while presenting one normal tuisnap
facade:

~~~text
crates/
  tuisnap-core/       validated data, comparison policies, query evaluation
  tuisnap-render/     pinned fonts, rendering, pixel comparison
  tuisnap-runtime/    pipes, PTY, lifecycle, shell/protocol integration
  tuisnap/            public API, Ratatui and Insta integration
  tuisnap-cli/        capture, inspect, review, report, protocol
~~~

The core model separates ProcessOutput, Screen, Observation, TerminalProfile,
RenderProfile, ComparisonPolicy, and ArtifactSet. Unknown and unsupported
properties must stay visible; they cannot become a plausible default that passes
a check. Terminal emulation and pixel rendering are separate concerns. The
official Ghostty binding is a candidate backend to qualify, not an approved
implementation or a claim of compatibility.

## Proposed user experience

The common pure-view path should be close to:

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

The names are proposed. The current equivalents are draw_frame, Renderer, and
explicit store checks. Proposed assert_snapshot! must compare styled canonical
state, not Frame::text(). Proposed assert_screenshot! must bind canonical
state, pixels, profile, and evidence to one capture and create failure artifacts
before asserting.

The piped process API must preserve stdout/stderr bytes separately, stdin EOF,
signals, exit status, timeouts, output limits, and deadlock-safe draining. It
must resolve an already-built executable and must not invoke Cargo per test.

The interactive API must own cleanup on normal return, error, panic, and
cancellation. Locators must resolve against the current observation revision,
require unique targets for actions, and retry observations without replaying
destructive actions. Semantic roles/IDs are opt-in application data and must
select real terminal input rather than bypassing the application.

## Release order and proof gates

The plan's milestones remain authoritative:

| Milestone | Required proof |
|---|---|
| M0 verification correctness | Mutations for changed pixels, missing/corrupt references, report disagreement, and mixed approval fail |
| M1 pure view testing | Production Ratatui closure/buffer adapters compile without PTY/runtime/native-engine dependencies |
| M2 Insta + nextest | Filtered, sharded, retried, relocated, and parallel consumer tests retain stable identities and evidence |
| M3 CLI + runtime | Piped process, PTY lifecycle, waits, input, backend capabilities, and cleanup corpus passes |
| M4 interaction | Locators, duplicate matches, stale revisions, negative waits, and action-once behavior are proven |
| M5 visual diagnosis | Pinned rendering, glyph/fallback policy, four-format artifacts, traces, and offline reports remain inspectable |
| M6 advanced parity | Named sessions, recording/replay, agent protocol, and optional clients pass an explicit capability matrix |
| M7 migration | Obsolete APIs/dependencies are removed; frozen approvals stay verified |

The first vertical slice is one pure settings view, one piped CLI error, and one
real settings-navigation journey. All three must run under nextest, produce
appropriate Insta expectations and readable failure evidence, and clean up
correctly. The fast-lane target is under 120 seconds on declared hardware; it is
a target, not a current measurement.

Qualification must test the tester with deliberate wrong glyph/style/continuation/
cursor, one changed pixel, missing font, changed palette, deleted baseline,
partial approval, skipped scenario, stale binary, full pipes, cancellation,
repeated destructive input, and retry evidence-overwrite mutations. Historical
frozen references stay unchanged during renderer/API migration.

## Unverified claims and next evidence

The following remain explicitly unverified as of this report:

- the proposed APIs and package split are not implemented;
- no native Insta compound snapshot transaction has been built;
- no cargo-nextest consumer qualification has been performed;
- no piped CLI process API exists in the current crate;
- the Ghostty binding is not qualified against the required corpus or build matrix;
- current termlens/termpane migration options are not selected;
- renderer output is deterministic under the checked-in profile by source tests,
  but pixel identity with a user's terminal has not been demonstrated;
- no performance benchmark or two-minute CI measurement was taken here;
- no cross-platform conformance, hard-kill containment, graphics compositing,
  MCP, or non-Rust client proof exists;
- this refresh did not run tests or benchmarks.

The next implementation action is therefore M0 mutation proof, followed by the
pure-view + Insta + nextest vertical slice. Do not claim broad competitor parity
or production readiness before those gates pass.

## Source references

All project-source links below are pinned to the inspected revision unless noted.

- [S1] Current manifest, features, and dependencies: [Cargo.toml](https://github.com/donbeave/tui-snap/blob/9dc86daff1dcbf20805b145916e8f04e9515f929/Cargo.toml)
- [S2] Current public facade and exports: [src/lib.rs](https://github.com/donbeave/tui-snap/blob/9dc86daff1dcbf20805b145916e8f04e9515f929/src/lib.rs)
- [S3] Ratatui buffer/TestBackend adapters: [src/ratatui.rs](https://github.com/donbeave/tui-snap/blob/9dc86daff1dcbf20805b145916e8f04e9515f929/src/ratatui.rs)
- [S4] PTY session, waits, input, cleanup boundary: [src/pty.rs](https://github.com/donbeave/tui-snap/blob/9dc86daff1dcbf20805b145916e8f04e9515f929/src/pty.rs)
- [S5] Raw replay through termpane: [src/ansi.rs](https://github.com/donbeave/tui-snap/blob/9dc86daff1dcbf20805b145916e8f04e9515f929/src/ansi.rs)
- [S6] Frame v3 schema and strict validation: [src/frame.rs](https://github.com/donbeave/tui-snap/blob/9dc86daff1dcbf20805b145916e8f04e9515f929/src/frame.rs)
- [S7] Pinned renderer and artifact generation: [src/render.rs](https://github.com/donbeave/tui-snap/blob/9dc86daff1dcbf20805b145916e8f04e9515f929/src/render.rs)
- [S8] Profile, fonts, fallback hashes, and geometry: [src/profile.rs](https://github.com/donbeave/tui-snap/blob/9dc86daff1dcbf20805b145916e8f04e9515f929/src/profile.rs)
- [S9] Current CLI commands and formats: [src/main.rs](https://github.com/donbeave/tui-snap/blob/9dc86daff1dcbf20805b145916e8f04e9515f929/src/main.rs)
- [S10] Renderer qualification assertions: [tests/render.rs](https://github.com/donbeave/tui-snap/blob/9dc86daff1dcbf20805b145916e8f04e9515f929/tests/render.rs)
- [S11] Classic store, statuses, comparison, and acceptance: [src/snapshot.rs](https://github.com/donbeave/tui-snap/blob/9dc86daff1dcbf20805b145916e8f04e9515f929/src/snapshot.rs)
- [S12] Grouped four-artifact store and fast/report paths: [src/grouped.rs](https://github.com/donbeave/tui-snap/blob/9dc86daff1dcbf20805b145916e8f04e9515f929/src/grouped.rs)
- [S13] Pixel comparison shortcut: [src/diff.rs](https://github.com/donbeave/tui-snap/blob/9dc86daff1dcbf20805b145916e8f04e9515f929/src/diff.rs)
- [S14] Snapshot, grouped, and qualification assertions: [tests/snapshot.rs](https://github.com/donbeave/tui-snap/blob/9dc86daff1dcbf20805b145916e8f04e9515f929/tests/snapshot.rs), [tests/grouped.rs](https://github.com/donbeave/tui-snap/blob/9dc86daff1dcbf20805b145916e8f04e9515f929/tests/grouped.rs), [tests/tool_qualification.rs](https://github.com/donbeave/tui-snap/blob/9dc86daff1dcbf20805b145916e8f04e9515f929/tests/tool_qualification.rs)
- [S15] Proposed architecture and release gates: [docs/REDESIGN-PLAN.md](https://github.com/donbeave/tui-snap/blob/dad80a81a930907e76c5ed1c1f279529bf05165c/docs/REDESIGN-PLAN.md), [docs/REDESIGN-BACKLOG.md](https://github.com/donbeave/tui-snap/blob/dad80a81a930907e76c5ed1c/docs/REDESIGN-BACKLOG.md)
- [S16] tui-test feature/API and serializer: [README.md](https://github.com/microsoft/tui-test/blob/7afb14b3c4075d24a7b9bf1a05175717f253821c/README.md), [snapshot serializer](https://github.com/microsoft/tui-test/blob/7afb14b3c4075d24a7b9bf1a05175717f253821c/crates/tui-test/src/assert/snapshot.rs)
- [S17] Terminal Control feature surface: [README.md](https://github.com/anomalyco/terminal-control/blob/c1d4f95e4f1b7638f6229e9bfc9599a955f95ce2/README.md)
- [S18] Terminal Control extraction behavior: [terminal_core.rs](https://github.com/anomalyco/terminal-control/blob/c1d4f95e4f1b7638f6229e9bfc9599a955f95ce2/src/terminal_core.rs)
- [S19] Terminal Control renderer and semantic protocol: [render.rs](https://github.com/anomalyco/terminal-control/blob/c1d4f95e4f1b7638f6229e9bfc9599a955f95ce2/src/render.rs), [semantic-protocol.md](https://github.com/anomalyco/terminal-control/blob/c1d4f95e4f1b7638f6229e9bfc9599a955f95ce2/docs/semantic-protocol.md)
- [S20] Insta public comparator: [comparator.rs](https://github.com/mitsuhiko/insta/blob/064742e9b7b2f3eaabb4724069e739ddf23d8227/insta/src/comparator.rs)
- [S21] Insta update policy documentation: [advanced configuration](https://insta.rs/docs/advanced/)
- [S22] Cargo Insta review and nextest workflow: [CLI documentation](https://insta.rs/docs/cli/)
