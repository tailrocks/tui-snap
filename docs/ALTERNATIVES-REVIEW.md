# Alternatives review: terminal testing tools and tui-snap

Status: refreshed review. Reverified 2026-09-28 against the current workspace, official project documentation, and the pinned source revisions below.

This document supersedes the former root `ALTERNATIVES-REVIEW.md`. The former document described an earlier v0.1 design. Claims about `BLESS`, geometry-only PNG output, and the old dependency arrangement are **legacy** and must not guide implementation. The current implementation and the proposed redesign are kept separate below.

## Decision

Keep tui-snap and redesign it as a Rust-first terminal testing platform. No reviewed alternative currently combines the required three modes behind one Rust API and one comparison model:

1. ordinary piped CLI processes;
2. real interactive terminal applications; and
3. detached production Ratatui views.

The proposed product uses native Insta expectations and runs as ordinary Rust tests under cargo-nextest. It borrows useful interaction, session, and diagnostic ideas from tui-test and Terminal Control without becoming a wrapper around either project or maintaining modified copies of their engines.

The recommendation is architectural, not a claim of shipped parity. The implementation sequence and proof requirements are in [REDESIGN-PLAN.md](REDESIGN-PLAN.md) and [REDESIGN-BACKLOG.md](REDESIGN-BACKLOG.md).

## Scope and evidence rules

The comparison answers which tool should own each testing concern. A renderer, transcript recorder, or CLI assertion library is an alternative for one layer; it is not automatically an alternative to the whole product.

Feature claims use one of these scopes:

- **Shipped**: verified in this workspace at the current branch head.
- **Source-reviewed**: observed in the pinned upstream source listed in the source register.
- **Proposed**: required by the redesign, but not implemented here.
- **Legacy**: retained only to explain why an earlier recommendation was superseded.

No benchmark, cross-platform qualification, or complete competitor corpus was run for this review. Performance and parity remain release-gated work.

## What tui-snap provides today

The current workspace is `tuisnap` 0.2.0. Its shipped surface includes:

- a direct Ratatui capture path that renders production view closures without a child process;
- an optional PTY path for real executables, keyboard/mouse/resize input, waits, and terminal-state capture;
- one canonical frame schema shared by capture and rendering;
- deterministic, pinned-font PNG output with fallback-glyph diagnostics;
- ANSI, TXT, JSON, SVG, HTML, and PNG export paths;
- fail-closed approval with explicit `accept`, rather than automatic `BLESS` updates;
- classic frame stores and a grouped four-artifact store;
- pure view builds without the PTY feature.

The proposed redesign is not shipped. In particular, the following remain planned: one complete `Observation` model, first-class `ProcessOutput`, native Insta compound assertions, nextest-aware attempt evidence, comprehensive locators, an owned session API, a qualified emulator backend, and the full CLI/protocol surface.

## Comparison at a glance

| Alternative | Best fit | What it proves | Boundary against the redesign |
|---|---|---|---|
| Ratatui `TestBackend` + Insta | Fast widget and layout tests | A production render function produced an expected in-process buffer | No PTY, process, event-loop, terminal-mode, or pixel contract |
| `expect-test` | Small inline golden values | A string or debug representation remains stable | Generic; no terminal interaction or rendering policy |
| `assert_cmd` | One-off piped CLI assertions | Exit status and stdout/stderr predicates | Does not exercise TTY behavior or fullscreen interaction |
| `trycmd` | Large declarative CLI case sets | Many command cases and text snapshots | Pipe-oriented; not a screen or PTY harness |
| `tui-test` | Rich real-terminal interaction and diagnostics | Locators, waits, input, state, snapshots, traces, and recordings | Broad session tooling; not centered on direct production Ratatui fixtures, this schema, Insta, or nextest |
| Terminal Control | Named sessions, semantic state, recordings, agent workflows | Real session control, retained frames, semantics, and media | Native backend/build boundary and a lossy presentation extraction must be qualified before reuse |
| `termlens` | Small Rust PTY integration tests | A real process rendered through a VT screen grid | No complete multi-mode product, pinned renderer, or four-format baseline workflow |
| `ratatui-testlib` / `terminal-testlib` | PTY tests with graphics or Bevy integration | PTY behavior, interactions, and selected graphics assertions | Additional framework and platform scope; no unified Insta/nextest/evidence contract |
| `term-transcript` | Static CLI/REPL transcript tests and docs | Text/SGR output matches a self-contained SVG transcript | Explicitly limited for cursor-moving fullscreen TUIs and general terminal state |
| `freeze` | Publication-quality still images | A command or captured output rendered as PNG/SVG/WebP | Renderer only; no assertions, approval, waits, or interaction |
| VHS / asciinema tooling | Reproducible demos and recordings | A scripted or recorded terminal timeline | Motion media, not a strict screen verification engine |

The closest building blocks are complementary. The redesign should compose the useful contracts while keeping the core smaller than a general terminal-control service.

## Layer A: pure Ratatui and snapshot alternatives

### Ratatui `TestBackend` plus Insta

Ratatui's official testing recipe renders a widget or application into `TestBackend` and passes the backend to Insta. It is the fastest baseline for layout and widget refactors. The recipe also states its boundary: `TestBackend` does not cover the event loop, key handling, terminal setup/teardown, or exit codes. The recipe's snapshot example is a buffer representation and currently documents that color assertions are not supported by that recipe.

Use this pattern in the first view-test milestone. The redesign should make the production draw closure and validated styled `Screen` equally easy to capture, then use native Insta storage and review. A screen assertion must preserve styles, geometry, continuations, and the documented cursor policy; it cannot silently reduce to plain text.

### `expect-test`

`expect-test` is useful for small inline values and compiler-style golden tests. It is generic and has no PTY, terminal state, renderer, or image contract. It remains a complementary dependency for tiny projections, not a foundation for tui-snap's screen model.

## Layer B: ordinary CLI process alternatives

### `assert_cmd`

`assert_cmd` locates built Rust binaries and asserts command results. It is a good interoperability target for the proposed `tuisnap::Command` API. Its pipe-based process model is exactly what is needed for non-interactive CLI contracts, including behavior that changes when stdout is not a terminal.

The redesign should preserve separate stdout and stderr bytes, stdin EOF behavior, raw non-UTF-8 data, exit/signal classification, limits, timeouts, and deadlock-safe draining. It should not route this mode through a PTY merely to share implementation.

### `trycmd`

`trycmd` enumerates declarative command cases and snapshots stdout/stderr files. It is strong for broad help, error, and argument matrices, including cases embedded in documentation. It does not model screen geometry, terminal modes, input choreography, or interactive redraws.

The proposed CLI snapshot assertion can reuse compatible conventions and remain interoperable with these tools. It should not replace them for teams that only need ordinary CLI coverage.

## Layer C: real interactive terminal alternatives

### `termlens`

The upstream project presents a focused model: spawn a real program in a PTY, feed typed input, wait on rendered screen predicates, and snapshot the resulting grid. Its documented waits and screen-based assertions are useful evidence for the redesign's runtime API.

The current workspace carries a local `vendor/termlens` source plus a separate `termpane` dependency. That is a current implementation fact, not the target ownership model. The redesign backlog requires a qualified migration to upstream PTY/emulator boundaries and forbids repaired or embedded competitor source copies. The planned `tuisnap-runtime` must add process ownership, atomic observations, capability reporting, nextest identity, and evidence journaling around upstream primitives.

### `tui-test`

The pinned project demonstrates the broadest interaction surface in this comparison: persistent sessions, text and style locators, locator composition, keyboard and mouse operations, resize and signal operations, explicit waits, snapshots, screenshots, recordings, and structured failure artifacts. Its Rust, Python, JavaScript, and CLI interfaces also show how one operation model can serve several callers.

Borrow:

- fresh locator resolution against the current screen;
- unique-target rules and explicit occurrence selection;
- retryable observational assertions;
- rich input operations and terminal-state inspection;
- readable and machine-readable failure evidence.

Do not copy its snapshot semantics without qualification. The redesign requires missing-reference and unsupported states to fail explicitly, and requires the assertion contract to prove that the state being exposed is the state being compared. tui-snap should keep one Rust core and make other clients optional adapters over that core.

### Terminal Control

The pinned project demonstrates named sessions, retained final screens, explicit capture reasons, recording/replay, a machine protocol, live human/agent interaction, and optional application-provided semantics. These are the right references for the advanced CLI and agent milestones.

Its current native build requirements and backend ownership must remain optional from pure view consumers. Its frame extraction is a presentation-oriented projection: the reviewed implementation resolves colors to RGB, applies inverse by swapping resolved colors, skips continuation/spacer cells, and collapses underline distinctions. That is unsuitable as the canonical source-state model for exact verification. tui-snap should preserve source distinctions in `Screen` and expose lossy projections only as explicit render or export policies.

### `ratatui-testlib` / `terminal-testlib`

This project targets PTY integration with keyboard/mouse events, waits, Insta snapshots, graphics protocols such as Sixel, Bevy ECS support, and headless execution. It is relevant when an application needs graphics-protocol or engine-specific integration coverage. Its current documentation describes a broader framework with additional platform and async concerns, so each required capability would need independent qualification before becoming a tui-snap dependency.

It is a useful compatibility fixture and source of test cases. It is not a reason to add Bevy, Sixel, or an alternate async runtime to the minimal view path.

### `rexpect` / `expectrl`

Expect-style libraries are appropriate for prompt/response flows and byte-stream matching. They are weaker for fullscreen TUIs because byte matches do not express the current screen, cursor, styles, scrollback, or repaint boundaries. Keep them as a valid choice for simple interactive CLIs, not as the screen observation model.

## Layer D: transcript, renderer, and recording tools

### `term-transcript`

`term-transcript` makes a static SVG transcript both documentation and a test oracle. It captures CLI/REPL interaction, embeds ANSI-compatible color information, can parse the SVG back, and can test text or text plus colors. Its own documentation limits non-SGR escape handling and warns that pipe capture changes `isatty` and terminal-size behavior; its optional PTY mode is not intended for complex cursor-moving fullscreen output.

Borrow the idea that a reviewed transcript can be readable documentation. Keep it as a specialized CLI/transcript option, not the canonical model for interactive terminal applications.

### `freeze`

`freeze` is a strong image generator for code and terminal output, with PNG, SVG, and WebP output and extensive presentation controls. It is a renderer, not a test framework: it does not provide the observation model, assertions, baseline approval, wait contracts, or nextest evidence identity required here. It remains useful for publication images when strict test evidence is not the goal.

### VHS and asciinema tooling

VHS defines terminal actions and renders GIF, MP4, WebM, or frame sequences. Asciinema and related tools provide a recording ecosystem. They are valuable for demos, replay, and communication. Time-based media is not a substitute for deterministic screen assertions: a recording may show what happened without proving a particular canonical state, pixel policy, or exit contract.

The redesign can add recording and media export after the core milestones. Those features must reuse the same operation protocol and evidence model rather than becoming a second assertion engine.

## Recommended division of responsibility

| Concern | Use today | tui-snap redesign direction |
|---|---|---|
| Widget/layout feedback | Ratatui `TestBackend` + Insta | Direct production-view `Screen` capture with native Insta structural assertions |
| Small inline values | `expect-test` | Optional projection only |
| Broad piped CLI matrix | `assert_cmd` or `trycmd` | `ProcessOutput` plus `assert_cli_snapshot!`, interoperable with both |
| Real PTY smoke journeys | `termlens`, tui-test, or a focused harness | Owned `Tui` runtime with explicit backend capabilities and cleanup |
| Locators and rich input | tui-test as reference | One Rust locator/action engine over current observations |
| Sessions and agent workflows | Terminal Control as reference | Optional CLI/protocol adapters over the same core |
| Publication screenshots | freeze or terminal-svg | tui-snap deterministic renderer for evidence; external renderers remain valid for marketing/docs |
| Recordings and demos | VHS/asciinema | Later optional recording/export milestone |

## What to retain and what to replace

Retain the current project's direct production-view capture, canonical grid, explicit font resources, offline rendering, missing-glyph diagnostics, fail-closed approval, and support for ANSI/TXT/PNG/HTML evidence. Those capabilities are useful differentiators and are already backed by workspace tests and committed references.

Replace the current API and dependency boundaries as the redesign milestones prove them. Do not preserve a cell-equality shortcut, approved-image reuse as candidate evidence, report/check disagreement, per-file mixed approval, or an assertion lifecycle that is easy to ignore. These are correctness defects identified in the plan, not alternative design choices.

Do not claim that tui-snap already has native Insta compound snapshots, complete nextest lifecycle management, locators, semantic providers, a qualified Ghostty backend, or competitor parity. Those are proposed and acceptance-gated.

## First vertical slice

The first proof should cover one pure settings view, one piped CLI error case, and one real settings-navigation journey. All three should run as ordinary tests under nextest, use appropriate Insta expectations, preserve readable failure evidence, and clean up owned processes. This establishes the product boundary before advanced sessions, media, or agent protocol work.

## Source register

### Pinned research baseline

- [tui-snap README at `9dc86da`](https://github.com/tailrocks/tui-snap/blob/9dc86daff1dcbf20805b145916e8f04e9515f929/README.md)
- [tui-test README at `7afb14b`](https://github.com/microsoft/tui-test/blob/7afb14b3c4075d24a7b9bf1a05175717f253821c/README.md)
- [Terminal Control README at `c1d4f95`](https://github.com/anomalyco/terminal-control/blob/c1d4f95e4f1b7638f6229e9bfc9599a955f95ce2/README.md)
- [Terminal Control frame extraction at `c1d4f95`](https://github.com/anomalyco/terminal-control/blob/c1d4f95e4f1b7638f6229e9bfc9599a955f95ce2/src/terminal_core.rs)
- [Terminal Control renderer at `c1d4f95`](https://github.com/anomalyco/terminal-control/blob/c1d4f95e4f1b7638f6229e9bfc9599a955f95ce2/src/render.rs)
- [Insta comparator at `064742e`](https://github.com/mitsuhiko/insta/blob/064742e9b7b2f3eaabb4724069e739ddf23d8227/insta/src/comparator.rs)

### Current official documentation and project sources

- [Ratatui snapshot testing recipe](https://ratatui.rs/recipes/testing/snapshots/)
- [Ratatui testing overview](https://ratatui.rs/recipes/testing/)
- [Insta snapshot types](https://insta.rs/docs/snapshot-types/)
- [Cargo Insta review](https://insta.rs/docs/cli/)
- [`assert_cmd` documentation](https://docs.rs/assert_cmd/latest/assert_cmd/)
- [`trycmd` documentation](https://docs.rs/trycmd/latest/trycmd/)
- [`expect-test` repository](https://github.com/rust-lang/expect-test)
- [`termlens` repository](https://github.com/vyncint/termlens)
- [`ratatui-testlib` repository](https://github.com/raibid-labs/ratatui-testlib)
- [`term-transcript` documentation](https://docs.rs/term-transcript/)
- [`freeze` repository](https://github.com/charmbracelet/freeze)
- [VHS repository](https://github.com/charmbracelet/vhs)

The pinned revisions describe the research comparison. Current upstream pages may add features after those revisions; such changes require a new source review before they become a tui-snap dependency or a parity claim.
