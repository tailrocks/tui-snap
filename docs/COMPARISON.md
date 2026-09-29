# Comparison with related projects

Inspected 2026-09-29. Pins are exact SHAs verified via `git
ls-remote` + the GitHub commits API; every claim cites source
fetched at the pinned SHA. Rust API and CLI are compared
separately. Strengths are honest: each project below does something
this toolkit does not.

## microsoft/tui-test

Pinned: `7afb14b3c4075d24a7b9bf1a05175717f253821c` (2026-09-24,
dependabot `build(deps)` #250). Workspace: `crates/tui-test` (Rust
lib) + `crates/tui-test-cli` (CLI) + `bindings/js`,
`bindings/python`.

**Rust API** (`crates/tui-test/src/`): PTY spawn (`terminal/pty.rs`
`Pty::spawn`, `runtime.rs` `Session::open/run`); string-token key
input (`input/keys.rs` `token_to_seq`, `KeyAction::{Press,Down,
Repeat,Up}`); SGR mouse (`input/mouse.rs`
`click/down/up/motion/drag_motion/scroll`); rich waits
(`api.rs::Operation::WaitTitle/WaitClipboard/WaitIdle/WaitCommand/
WaitExit/WaitReady/WaitBell/WaitLocator`); expect-* assertions plus
Playwright-style locators (`runtime.rs::Locator`
`get_by_text/get_by_style/get_by_link`, `and/or/filter`,
`first/last/nth`, `wait`, `click`, `expect`, `highlight`);
snapshot compare (`assert/snapshot.rs` `SnapshotStatus`, `compare`,
`serialize`); screenshots in SVG **and PNG** (`render/encode.rs`
`encode_png`, `render/svg.rs` `GridRenderer`); APNG/GIF/MP4/
asciicast recording (`record.rs`, `trace/recorder.rs`). Sync API
over `TuiTestError`. Four pluggable emulator backends
(`terminal/backend.rs::Backend::{Alacritty,Ghostty,Rio,Xtermjs}`).
Tests: `tests/runtime.rs`, `tests/diagnostic_failures.rs`,
`src/render/snapshot_tests.rs`, `src/render/raster/tests.rs`.

**CLI** (`tui-test`, `crates/tui-test-cli/src/cli.rs`, 1652 lines,
clap): `open/run/restart/close/sessions/daemon/state/text/
screenshot/record/cells/get/type/submit/key/press/mouse/resize/
write/signal/kill/wait/expect/find/click/highlight/get-recording/
monitor/usage/agent-context/skill/internal-daemon/start/stop`.
Screenshot: `--out/--full/--zoom/--background/--transparent` (SVG
or PNG by path). Record: `record start|stop`,
`--format/--fps/--speed/--idle-time-limit/--zoom`. Daemon-backed
(`daemon.rs`, `ipc.rs`, `protocol.rs`), agent-oriented
(`agent_context.rs`, `skill.rs`, root `SKILL.md`).
Tests: `crates/tui-test-cli/tests/session_lifecycle.rs`.

**Honest strengths vs this toolkit:** (a) the only competitor with
true Playwright-style locators + expect/wait vocabulary in Rust;
(b) 4 emulator backends for conformance testing; (c) PNG *and* SVG
screenshots plus video recording in one tool; (d) daemon + session
multiplexing + JS/Python bindings = multi-agent/multi-language
story; (e) richest structured failure diagnostics (HTML+markdown
reports, trace bundles).

**Where this toolkit differs:** explicit approval workflow with no
ambient bless (fail-closed stores, generation-bound compound
gates); pinned render profile with vendored-font determinism
(byte-identical PNGs); typed op protocol + MCP bridge without a
daemon; pure-view path with zero subprocess.

## anomalyco/terminal-control

Pinned: `c1d4f95e4f1b7638f6229e9bfc9599a955f95ce2` (2026-09-04,
`chore: release terminal-control 1.2.1 (#34)`). Single crate
`terminal-control` 1.2.1 + bin `termctrl`. Deps: `portable-pty`,
`libghostty-vt`, `crossterm`, `resvg`, `rmcp`, `tokio`, `clap`.

**Rust API** (`src/`, public surface per `lib.rs` + documented in
`docs/rust-library.md`): PTY spawn (`session.rs::Session::start`,
portable-pty); key input is **raw bytes only** (`send(&[u8])` — the
`Key` enum in `input.rs` is `pub(crate)`, no chords); mouse yes
(`mouse.rs::MouseEvent`, zero-based cells, Down/Move/Up drags);
waits (`wait_for_text/idle/exit`); capture + `Frame::text()` +
semantic JSON with NO built-in assertions (caller asserts);
`Shot`/`Frame` are serializable structs (`FORMAT_VERSION=2`) but
there is **no snapshot-assert/macro integration**; screenshots in
PNG (via resvg) + SVG (`render.rs::svg/png`,
`render/box_drawing.rs`); `.termctrl` JSONL recordings
(`recording.rs`: `Entry/InputOrigin/Writer`,
`shot_at(path,at_ms,marker)`, `video()`); MCP tools (`mcp.rs`:
`list_sessions/get_session_status/get_screen/save_screen/…`);
external-driver protocol (`driver.rs`, `PROTOCOL_VERSION=2`,
`docs/driver-protocol.md`). Sync API, `anyhow::Result`. Tests:
`tests/daemon_detach.rs` (unix-only), inline `KEY_CASES` in
`src/input.rs`; no broad integration suite at pin.

**CLI** (`termctrl`, `src/main.rs`, 1724 lines, clap):
`show/save/start/run/wait/send/mouse/status/list/prune/resize/
mark/markers/logs/restart/stop/video/driver/mcp/__serve(hidden)`.
Formats `ShotFormat::{Png,Svg,Txt,Json,Ansi,Semantic}`; render
flags `--cell-width/--cell-height/--padding/--font-family/
--pixel-ratio/--hide-cursor`; sources: named session, command,
ANSI pipe, `.termctrl` recording (`--at-ms`, markers). Daemon
model: named sessions over unix sockets in
`TERMCTRL_RUNTIME_DIR` (0o700, `src/runtime.rs`).

**Honest strengths vs this toolkit:** (a) `Shot` keeps `frame` +
source `ansi` together; (b) time-travel: `shot_at(recording,
at_ms|marker)` + `markers` + video export from one file; (c) unique
**semantic snapshot** channel (app-provided UI JSON via socket,
`TERMCTRL_SEMANTIC_SOCKET`) — asserts structure, not pixels;
(d) MCP server + external driver protocol = agent-native control
plane; (e) single-crate simplicity with copy-paste Rust examples.

**Where this toolkit differs:** real assertion gates (exact cells +
decoded pixels + Insta compounds) instead of capture-only; key
chords + mode-aware input encoding as public API; approval
lifecycle (accept/frozen/review) instead of save-and-compare-by-hand.

## vyncint/termlens

Pinned: `5daf54fbc18c0ee66bec371be56677f63f638d37` (2026-09-28,
dependabot `thiserror` 2.0.20→2.0.21 #533). Workspace:
`crates/termlens` (lib, "Playwright for the terminal") +
`crates/termlens-cli` (bin `termlens`) + `fixtures/*`.

**Rust API** (`crates/termlens/src/`): PTY spawn (real PTY +
background reader + query answering) via `Terminal::builder()
.size/timeout/arg(s)/env(s)/scrollback/record_budget/answer_queries
….spawn(program)`; key input (`keys.rs` `Key`, `Chord`, mode-aware
encoding; `send/send_after/send_str/paste`); mouse (click/drag/
scroll, mode-aware SGR); waits (`wait_until`, `wait_frame` on
DEC-2026 sync frames only, `wait_idle`, `wait_stable`,
`wait_until_matches` regex, `wait_exit`); `Screen` with 60+
accessors (`screen.rs`: NFC-folded `find/locate`, cursor/title/
modes/clipboard/links/repaints/bells/graphics/unsupported,
`mask_rect/mask_matching`, `with_styles`); `Screen::diff`
(`screen/diff.rs`), text round-trip (`Screen::parse`), serde JSON
round-trip; screenshots as text/ANSI/SVG/HTML/JSON/asciicast
(`screen/render.rs` `to_ansi/to_svg/to_html`) — **explicitly no
PNG** (zero `png|pixel|raster` hits in render/lib/CLI at pin);
`assert_screen_snapshot!` (insta feature, default): settle +
styles + snapshot in one macro; timeout errors embed the screen
(`error.rs::Error::Timeout`, `Error::screen()`). Sync API,
`termlens::Result`. Tests: 34 integration files (`tests/{basic,
artifact,backpressure,builder_validation,charset,compat,
concurrency,conpty_probe,drain,export,fixtures,frames,graphics,
input,insert_mode,inspect,observe,process,queries,readme_example,
record,scrollback,search,snapshot_macro,stable,state,
styled_history,styles,tabs,timeouts,unsupported,utf8,wrap}.rs` +
`tests/common/mod.rs` + `tests/compat/` corpus) — the richest
suite of the three.

**CLI** (`termlens`, `crates/termlens-cli/src/main.rs`, 782 lines,
hand-rolled parsing, no clap — a companion/debug tool, not a test
runner): `inspect [--size COLSxROWS] [--timeout S] [--idle MS]
[--cwd] [--env K=V] [--ansi] -- <program>` (run in PTY, print
screen + exit trailer); `diff [--color when]` (two saved screens,
cell-level paint); `render [--svg|--html|--ansi|--text|--json]
[--out]` (one saved screen). No session management, no input
injection, no PNG. Tests: `crates/termlens-cli/tests/cli.rs` (14
tests).

**Honest strengths vs this toolkit:** (a) deepest terminal-fidelity
model: sync-frame waits (no torn repaints), NFC matching,
wide-char/scrollback/insert-mode handling, graphics-protocol
counting, query answering; (b) out-of-band assertions others miss:
cursor shape, OSC-52 clipboard, OSC-8 links, repaints, bells;
(c) best snapshot ergonomics: one-macro settle+styles+snapshot with
insta, plus diff/mask/parse/serde round-trips; (d) most disciplined
test corpus (34 integration files + fixtures + compat evidence +
semver gates); (e) timeout errors carry the screen — CI-debuggable
without artifacts.

**Where this toolkit differs:** PNG pixel gates (termlens is
vector/text only by design); fail-closed approval lifecycle with
frozen policy and generation binding; a full CLI (sessions,
record, machine protocol) instead of a debug companion; vendored-
font render determinism.

## Cross-cutting summary

| Capability | this toolkit | tui-test | terminal-control | termlens |
|---|---|---|---|---|
| PTY spawn + key/mouse input | yes (chords, ModeNotEnabled) | yes (tokens, SGR) | PTY yes; keys raw-bytes-only | yes (Key/Chord, SGR) |
| Waits with evidence on timeout | yes | yes | yes (no assert glue) | yes (screen in error) |
| Cell-exact snapshot gates | yes (cells + ANSI) | yes (compare) | no (capture only) | yes (insta macro + diff) |
| PNG pixel gates | yes (decoded-pixel) | yes (encode_png) | yes (resvg) | **no (by design)** |
| SVG/HTML/text/ANSI export | yes | yes | yes | yes (no PNG) |
| Video/cast recording | cast/GIF/APNG export | APNG/GIF/MP4/cast | .termctrl + video | asciicast |
| Explicit approval lifecycle | yes (accept/frozen/review) | review via diagnostics | no | insta review |
| Agent control plane | op protocol + MCP, no daemon | daemon + MCP-ish skill + bindings | MCP + driver protocol | none |
| Test corpus at pin | 413 tests, 8-example lane | runtime + CLI suites | thin (daemon + unit) | 34 integration files |

No fabricated data: every SHA/date above came from `git ls-remote`
and the GitHub commits API on 2026-09-29; every symbol cites a file
fetched at the pinned SHA.
