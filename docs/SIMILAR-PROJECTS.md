# Similar projects

Status: reverified 2026-09-28 against current public repository heads.

This inventory supports the [Rust-first redesign plan](REDESIGN-PLAN.md). It separates user-facing terminal test platforms from adjacent components. It is a current comparison aid, not a claim that tui-snap already provides the proposed APIs or competitor parity.

## Direct competitors and closest overlaps

These projects overlap with at least one proposed tui-snap mode: pure view fixtures, piped CLI tests, or real interactive terminal sessions.

- [Microsoft tui-test](https://github.com/microsoft/tui-test/blob/7afb14b3c4075d24a7b9bf1a05175717f253821c/README.md) — Cross-platform CLI, Rust, Python, and JavaScript tooling for controlling, inspecting, testing, and recording shell and full-screen terminal sessions. It has locator-style queries, retryable assertions, screenshots, traces, and agent-facing workflows. The repository states that its current API is a beta rewrite. Its strongest overlap is interactive sessions and agent automation; it does not provide tui-snap's proposed first-class production Ratatui view fixture workflow.
- [anomalyco/terminal-control](https://github.com/anomalyco/terminal-control/blob/c1d4f95e4f1b7638f6229e9bfc9599a955f95ce2/README.md) — Rust CLI and library built around a Ghostty terminal core. It provides PTY capture, named sessions, keyboard/mouse input, waits, screen/JSON/ANSI/SVG/PNG evidence, semantic application integration, a JSON-lines driver, MCP, a TypeScript client, and recordings/video. Its strongest overlap is the interactive runtime and external automation. Its frame model remains a presentation-oriented source to qualify against tui-snap's complete source-state observation contract.
- [vyncint/termlens](https://github.com/vyncint/termlens/blob/5daf54fbc18c0ee66bec371be56677f63f638d37/README.md) — Rust PTY harness for CLI/TUI applications with an emulated screen, waits, terminal-state accessors, mode-aware input, and Insta snapshots. It is a close implementation reference for PTY lifecycle and truthful capability reporting. Its current README lists Linux, macOS, and Windows/ConPTY, with feature limitations on Windows; it also notes bounded scrollback and captured-but-not-composited graphics. It does not cover the proposed pure-view and piped-process APIs as one facade.
- [raibid-labs/ratatui-testlib](https://github.com/raibid-labs/ratatui-testlib/blob/9e85657724f6b5edaa7a87af057f7a59c4655bb5/README.md) — Rust PTY integration harness for Ratatui and other terminal applications, with Insta hooks, waits, input, graphics protocol support, and optional Bevy integration. It is a narrower application-focused harness, useful for PTY and graphics fixtures, rather than a complete three-mode platform.

## Adjacent foundations and components

These projects provide pieces tui-snap should integrate with or qualify, but they are not substitutes for the proposed testing facade.

- [Ratatui](https://github.com/ratatui/ratatui/blob/54b6874357764219168c63eef2da26935b9a9609/README.md) — The Rust TUI framework. `TestBackend`, buffers, and widget-level assertions support fast in-process view tests. They do not launch a real process or model a PTY session.
- [Insta](https://github.com/mitsuhiko/insta/blob/064742e9b7b2f3eaabb4724069e739ddf23d8227/README.md) — Rust snapshot storage, naming, review, structured snapshots, and public comparator APIs. It supplies the snapshot lifecycle; tui-snap must supply terminal observations, rendering, and the compound canonical-plus-image policy.
- [cargo-nextest](https://github.com/nextest-rs/nextest/blob/44d8af28816fcb5c364e701c211fb32d456ea31a/README.md) — Rust test runner with filtering, retries, test groups, leak handling, JUnit output, stress repetition, and runtime executable metadata. It supplies execution and reporting primitives; it is not a terminal emulator or assertion engine.
- [Ghostty](https://github.com/ghostty-org/ghostty/tree/b1d2b7ef1f1cf5cc0118298bad849870bc678298) — Terminal emulator and parser candidate for a qualified tui-snap interactive backend. It is an upstream runtime component, not a testing framework. Native build requirements must stay out of pure view consumers.
- [term-transcript](https://github.com/slowli/term-transcript/blob/db1b3a780194984a3860592969a5f2527e88c0be/README.md) — Rust CLI/REPL transcript capture, SVG generation, SVG parsing, and transcript tests. It is useful for transcript documentation and output oracles, but it is not a general PTY screen-observation engine.
- [ansi-to-tui](https://github.com/ratatui/ansi-to-tui/tree/585c83c932c70cd0a9adec6270e1c5306a3929a3) — Converts ANSI-colored text into Ratatui text. It is a parsing/conversion utility, not process orchestration or terminal testing.
- [reg-cli](https://github.com/reg-viz/reg-cli/tree/b7a2b1e4603275698ad27a5cb8e46d704831e6f9) — Generic image visual-regression comparison and HTML reporting. It can inform diff/report design, but it has no terminal grid, PTY, or Ratatui semantics.

## Rendering and recording tools

These are useful evidence or demo references. They should not define tui-snap's canonical state, comparison, or lifecycle contracts.

- [Charm freeze](https://github.com/charmbracelet/freeze/blob/65acb1e1daed1e6f6989496e15a803b256fdda3a/README.md) — Generates styled PNG, SVG, and WebP images from code or ANSI terminal output. It is a renderer/screenshot utility, not a test harness.
- [termshot](https://github.com/homeport/termshot/tree/cfaadac0cc0a471623a67a29b3e5af9442f9aebd) — Generates screenshots from terminal command output. It does not provide the proposed observation model or snapshot lifecycle.
- [termframe](https://github.com/pamburus/termframe/tree/a5f41147791485ca9f2cd2e3426a1497671a86f1) — Converts terminal output to SVG. It is a still-image renderer, not an interactive test runtime.
- [terminal-svg](https://github.com/russmckendrick/terminal-svg/tree/b98a39dbaf0dc999f45a0d6660fc8203f21874d2) — Produces self-contained SVG screenshots from terminal output. It is adjacent rendering infrastructure.
- [Charm VHS](https://github.com/charmbracelet/vhs/blob/24fa2254a9806091e6ee6a980e9f3bcfe0a9ba53/README.md) — Scripted terminal tapes for CLI integration examples and GIF/MP4/WebM/PNG output. It is useful for deterministic interaction scripts and demos; its tape timing and raster output are not a replacement for assertion semantics.
- [asciinema](https://github.com/asciinema/asciinema/blob/77498061982889f68ba1f6e8b666dd8d13246250/README.md) — Terminal session recording, playback, conversion, and streaming using asciicast files. It is a recording format/tool, not a screen-state or snapshot oracle.

## Historical and renamed references

- [`kitlangton/cellshot`](https://github.com/kitlangton/cellshot) is no longer a separate repository: the old URL redirects to [`anomalyco/terminal-control`](https://github.com/anomalyco/terminal-control). The old root document's description of “cellshot” as a separate strongest rival is stale. Current competitor analysis should use the maintained repository that applies to the comparison, especially [Terminal Control](https://github.com/anomalyco/terminal-control) at the pinned revision above.
- The previous root document listed several GIF/SVG recording converters and a recorder index as though they were terminal testing peers. They remain possible export references, but their lack of a shared observation/assertion model makes them irrelevant to the core architecture. They are intentionally omitted from the current competitor set.

## Reverification heads

The following heads were checked with `git ls-remote` on 2026-09-28. These hashes identify the source used for the status summaries above; the links intentionally point to the maintained repositories and current documentation.

| Project | HEAD |
|---|---|
| [tui-test](https://github.com/microsoft/tui-test) | `7afb14b3c4075d24a7b9bf1a05175717f253821c` |
| [anomalyco/terminal-control](https://github.com/anomalyco/terminal-control) | `c1d4f95e4f1b7638f6229e9bfc9599a955f95ce2` |
| [termlens](https://github.com/vyncint/termlens) | `5daf54fbc18c0ee66bec371be56677f63f638d37` |
| [ratatui-testlib](https://github.com/raibid-labs/ratatui-testlib) | `9e85657724f6b5edaa7a87af057f7a59c4655bb5` |
| [Insta](https://github.com/mitsuhiko/insta) | `064742e9b7b2f3eaabb4724069e739ddf23d8227` |
| [cargo-nextest](https://github.com/nextest-rs/nextest) | `44d8af28816fcb5c364e701c211fb32d456ea31a` |
| [Ratatui](https://github.com/ratatui/ratatui) | `54b6874357764219168c63eef2da26935b9a9609` |
| [term-transcript](https://github.com/slowli/term-transcript) | `db1b3a780194984a3860592969a5f2527e88c0be` |
| [freeze](https://github.com/charmbracelet/freeze) | `65acb1e1daed1e6f6989496e15a803b256fdda3a` |
| [VHS](https://github.com/charmbracelet/vhs) | `24fa2254a9806091e6ee6a980e9f3bcfe0a9ba53` |
| [asciinema](https://github.com/asciinema/asciinema) | `77498061982889f68ba1f6e8b666dd8d13246250` |
| [Ghostty](https://github.com/ghostty-org/ghostty) | `b1d2b7ef1f1cf5cc0118298bad849870bc678298` |
| [ansi-to-tui](https://github.com/ratatui/ansi-to-tui) | `585c83c932c70cd0a9adec6270e1c5306a3929a3` |
| [reg-cli](https://github.com/reg-viz/reg-cli) | `b7a2b1e4603275698ad27a5cb8e46d704831e6f9` |
| [termshot](https://github.com/homeport/termshot) | `cfaadac0cc0a471623a67a29b3e5af9442f9aebd` |
| [termframe](https://github.com/pamburus/termframe) | `a5f41147791485ca9f2cd2e3426a1497671a86f1` |
| [terminal-svg](https://github.com/russmckendrick/terminal-svg) | `b98a39dbaf0dc999f45a0d6660fc8203f21874d2` |
