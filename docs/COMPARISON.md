# Comparison with related projects

Inspected 2026-09-29. Pins are exact SHAs verified via `git
ls-remote` + the GitHub commits API and re-verified against local
clones (`git rev-parse HEAD` matches on all three). Rust API and
CLI are compared separately per operation.

Epistemics: tuiscotti cells are **executed** (full nextest 519/519,
examples 01–08, and the README CLI chain all ran green at head
`75ff479` on 2026-09-29). Competitor cells are **source-cited at
the pinned SHA, not executed** — every symbol cites a file:line
confirmed to resolve in the clone; corrected pointers are used
where the first pass was off by a line. Nothing here is
run-or-it-didn't-happen for competitors; nothing is
prose-only for tuiscotti.

| Repo | Pinned SHA | Date | Version |
|---|---|---|---|
| microsoft/tui-test | `7afb14b3c4075d24a7b9bf1a05175717f253821c` | 2026-09-24 | 0.1.0-beta.5 |
| anomalyco/terminal-control | `c1d4f95e4f1b7638f6229e9bfc9599a955f95ce2` | 2026-09-04 | 1.2.1 |
| vyncint/termlens | `5daf54fbc18c0ee66bec371be56677f63f638d37` | 2026-09-28 | 0.11.4 |

## launch

| | tuiscotti (executed) | tui-test | termlens | terminal-control |
|---|---|---|---|---|
| Rust | `Tui::new(["./my-tui"]).size(120,40).spawn()?`; `Tui::cargo_bin(n)` (eager, typed error) | `Session::new("example")` + `open(OpenOptions)` / `run(RunOptions)` (`src/runtime.rs:423,432`, `src/api.rs:78,114`); daemon auto-starts, socket under `~/.tui-test` (`tui-test-cli/src/config.rs:52-56`) | `Terminal::builder().size(80,24).timeout(d).args([…]).spawn("myapp")?` (`src/terminal.rs:1865`, validated `:1801`); `bin!` macro pins `CARGO_BIN_EXE_` (`lib.rs:291-300`) | `Session::start(cmd, cwd, record, opts)` (`src/session.rs:218-225`); daemon via `Command __serve` + setsid + readiness poll (`:1240-1308`) |
| CLI | `tuiscotti session start --name N -- argv…`; `capture`/`record` for one-shots | `tui-test open/run/restart/close/sessions` (daemon-backed) | `termlens inspect -- <program>` only (debug companion, no sessions) | `termctrl start NAME -- …`, `run [NAME] -- …` (`src/main.rs:359-429`) |
| tradeoff | tuiscotti sessions are ephemeral and test-scoped (in-process, Drop reaps); tui-test/termctrl sessions persist via a daemon (agent reuse, `monitor` attach) at the cost of socket IPC, an idle watchdog, and flock-guarded lifecycle. termlens validates pre-spawn with remedy-naming errors; tuiscotti validates profile caps at spawn. |

## views (pure render, no subprocess)

| | tuiscotti (executed) | tui-test | termlens | terminal-control |
|---|---|---|---|---|
| Rust | `ratatui::draw_frame(120,40,prov, \|f\| …)`, `render_screen` → `Screen` — the production view, in-process | none; closest is viewport-vs-scrollback `full: bool` on `Text`/`PackedScreen`/`Screenshot` (`src/api.rs:633,636,774`) | none; one grid + history: `screen()`, `text()/scrollback_text()/full_text()` (`screen.rs:1314,1351,1449`) | none; one fullscreen `Frame` (`src/frame.rs:79-87`) |
| CLI | `render --input frame.json --format …` (offline, byte-identical) | n/a | `render [--svg\|--html\|--ansi\|--text\|--json]` of a saved screen | `show/save --pipe` (still captures from output, never in-process) |
| tradeoff | tuiscotti alone renders the production Ratatui view without a subprocess (deterministic, `--no-default-features` builds it with no PTY/native deps). All three competitors are PTY-only: every pixel goes through a spawned child + emulator. |

## pipes (piped stdio, no PTY)

| | tuiscotti (executed) | tui-test | termlens | terminal-control |
|---|---|---|---|---|
| Rust | `Command::new(a0).arg().env().stdin().timeout().output_limit().run() → ProcessOutput`; exit/signal/timeout/limit stay distinct (`Termination`) | none; raw byte injection via `Operation::Write` (`src/api.rs:656`) | none, PTY-only (PTY merges stdout/stderr by nature) | `from_pipe_command()`: piped stdout+stderr, no PTY (`src/shot.rs:196-292`) |
| CLI | `capture --out D -- prog…`, `record --out T -- prog…` (exit code preserved, manifest records termination) | `write` (injects into a PTY session) | none | `show/save --pipe -- CMD` (`main.rs:286-288,920-925`) |
| tradeoff | tuiscotti piped runs are first-class with a truthful termination taxonomy; termctrl has one-shot pipe capture but no timeout/output-limit/termination distinctions; tui-test/termlens cannot test piped stdio at all. |

## PTY engine

| | tuiscotti (executed) | tui-test | termlens | terminal-control |
|---|---|---|---|---|
| Rust | `portable-pty` 0.9 + `alacritty_terminal` 0.26, reader+worker threads, process-wide `PTY_LIFECYCLE` open guard | `portable-pty` (`src/terminal/pty.rs:56,63,80`) + `Emulator` trait with 4 backends: alacritty (default), ghostty, rio, xtermjs (`terminal/emu.rs:411`, `backend.rs:21,33`) | `portable-pty` 0.9 + `vt100` 0.16 + shadow parser (`src/emu/`); open retry + lifecycle lock (`terminal.rs:297-314`); reader attached before spawn (`:1930-1931`) | `portable-pty` spawn (`session.rs:260-290`) + Ghostty `libghostty-vt` wrapped thread-confined (`terminal_core.rs:18-26,97-112`); host replies for OpenTUI/Kitty/DA1-style queries (`shot.rs:491-630`; no DA1 handler exists in `src/`) |
| CLI | n/a (engine is a library detail; `doctor` reports `pty: true/false`) | n/a | n/a | n/a |
| tradeoff | tui-test's swappable backends enable conformance testing; termctrl's Ghostty needs Zig 0.15.2 + network at build (`README.md:14`); termlens's vt100 is the lightest. tuiscotti is single-backend by design (pure-cargo build, no git/path deps) — a G1 termpane-only swap is planned (sibling-owned), not landed. |

## args (native child argv)

| | tuiscotti (executed) | tui-test | termlens | terminal-control |
|---|---|---|---|---|
| Rust | `Tui::arg/args`, `Command::arg/args` (`AsRef<OsStr>` throughout) | straight to `CommandBuilder` (`pty.rs:63,80`) | `.arg()/.args()` (`terminal.rs:1529,1536`), `spawn(impl AsRef<OsStr>)` (`:1865`) | `builder.args(&command[1..])` (`session.rs:272-273`) |
| CLI | `capture/record/session start … -- prog args…` (`last = true`, never parsed as tool flags) | `run`: trailing + hyphen passthrough (`cli.rs:266`), auto-inserts `run` after `--` | `inspect <program> [args..]` | clap trailing + hyphen passthrough on all command fields (`main.rs:329-330`) |
| tradeoff | rough parity everywhere: native `OsStr` argv with `--` passthrough. tui-test additionally rewrites argv to infer `run`. |

## config

| | tuiscotti (executed) | tui-test | termlens | terminal-control |
|---|---|---|---|---|
| Rust | `Profile::default_profile()` (render pins); `TerminalProfile` (terminal caps); `tuiscotti.toml` read by tests via the Rust API | `tui-test.toml` (cwd → config-home → `~/.tui-test`, `TUI_TEST_CONFIG` override; `src/profile.rs:34,458,497`): profiles + timeouts + recording + diagnostics + trace (`:353`); per-class `TUI_TEST_TIMEOUT_*_MS` env | builder fields only (`terminal.rs:1441-1481`): 80×24, 5 s timeout, `TERM=xterm-256color`, scrollback 1000, record budget 2M cells; `TERMLENS_ARTIFACT_DIR` env (`error.rs:180`) | per-call `Options{cols,rows,cell_*,settle,deadline,…}` (`shot.rs:22-60`); env `TERMCTRL_RUNTIME_DIR`, `TERMCTRL_SEMANTIC_SOCKET` |
| CLI | `tuiscotti init [--dir] [--force]` scaffolds `tuiscotti.toml` + nextest config + example test | config file discovered implicitly | flags only | flags only |
| tradeoff | tui-test's one file covers timeouts/recording/diagnostics/trace; tuiscotti splits by owner (`tuiscotti.toml` capture policy vs nextest scheduling vs Insta review — printed by `init`, pinned in `proto::CONFIG_DOCS`). termlens/termctrl are file-less: every knob is a flag/builder call, nothing shared across tests. |

## waits

| | tuiscotti (executed) | tui-test | termlens | terminal-control |
|---|---|---|---|---|
| Rust | `wait_predicate/wait_stable[_quiet]/wait_frame/wait_exit/expect_exit` + `_timeout` variants; `CancelToken`; `wait_frame` fails closed `Unsupported` (no DEC-2026); failures carry evidence | title, clipboard(+regex), idle, command, exit, ready, bell (wait ops `src/api.rs:676-706`); shell-integration `command`/`ready` via injected shell scripts | `wait_until[_for]`, `wait_frame[_for]` (real DEC-2026), `wait_idle[_for]`, `wait_stable[_for]` (returns `Screen`), `snapshot_after[_for]`, `wait_exit[_for]`, regex `wait_until_matches[_for]` (`terminal.rs:3278-4201`); 50 ms poll cap, 1→20 ms backoff (`wait.rs:14,22,80`) | `wait_for_text/wait_for_idle/wait_for_exit` (`session.rs:431-487`); driver `WaitForText/WaitForIdle/WaitForExit` |
| CLI | n/a (waits are API-level; `machine` has `wait`/`exit` ops with `text`/`stable`/`exit` kinds) | `wait …`, `expect …` (`cli.rs:1454-1455`) | none | `wait NAME TEXT --timeout`; one-shot `--wait-for/--settle-ms/--deadline-ms` |
| tradeoff | tui-test's shell-integration waits (`command`/`ready`) and termlens's real `wait_frame` + `snapshot_after` (predicate+settle fused) exceed tuiscotti; tuiscotti answers with explicit `CancelToken`, kitty-sync honesty (fail closed instead of faking a frame), and evidence-carrying timeouts. termctrl covers text/idle/exit only. |

## input

| | tuiscotti (executed) | tui-test | termlens | terminal-control |
|---|---|---|---|---|
| Rust | `send_text/send_bytes/paste/press/press_key/key_down/repeat/up/key_event`, `click/down/up/move/drag/wheel`, `focus_in/out`, `resize`, `signal`; mode-gated fail-closed (`ModeNotEnabled`) | `token_to_seq` incl. kitty encoding (`input/keys.rs:736`); SGR mouse builders (`input/mouse.rs:8,34`); encodes unconditionally | `send` (mode-aware DECCKM `:2715`), `send_after` (Esc disambiguation `:2766`), `send_str`, bracketed `paste`, `click/click_with`, per-cell `drag`, `scroll`, `focus_in/out`; `Key::encode`, `Chord` (`keys.rs:50,133,199,245`); refused → `Error::Input` | bytes `send/send_all` + pace (`session.rs:406-428`); wire enum `input.rs:6-44`; mouse `MouseEvent` zero-based (`mouse.rs:36-67`), Ghostty-encoded, errors if the app lacks reporting |
| CLI | n/a (API-level; `machine` has a `stdin` op: text/chord/base64-bytes) | `type/submit/key press/down/repeat/up/mouse click/move/down/up/drag/scroll` (`cli.rs:355-377`) | none | `send`, `mouse`, `--stdin` burst (`main.rs:442-455`) |
| tradeoff | near-parity on key/mouse surface. tuiscotti refuses mode-disabled input with a typed error (like termlens's `Error::Input`); tui-test encodes unconditionally. termlens adds `send_after` Esc-delay and per-cell drag; tuiscotti adds `+`-joined chord strings (`press("ctrl+Up")`); termctrl adds paced send. |

## locators

| | tuiscotti (executed) | tui-test | termlens | terminal-control |
|---|---|---|---|---|
| Rust | `Locator::{text,regex,style,region}` + combinators (`within/before/after/nth/first/last/and/or/filter/mode`); session-bound `get_by_text` → `BoundLocator::{expect_visible,click}` (minimal); detached `resolve/resolve_unique/prepare_action` | session-bound LAZY: re-resolved per read/wait/action (`runtime.rs:77-82`); cross-session combine rejected (`:101`); text literal/regex + whitespace mode + scope, style fg/bg/attrs, OSC-8 link, and/or/filter, relative dirs (`api.rs:336-542`); occurrence any/unique/first/last/nth (`runtime.rs:207`); `wait/wait_hidden/click/expect/highlight/locations/count/all` (`:242-336`) | immediate grid queries: `locate()`, `find/find_all/find_by`, regex `find_match/matches` (`screen.rs:1266-2167`); volatile-region masks `mask_rect/mask_matching/mask_cells` (`:1734-2167`); NFC-folded | NONE: only `terminal.text()?.contains(text)` (`session.rs:445`) |
| CLI | n/a | `find/click/highlight` | n/a (debug CLI has no query surface) | n/a |
| tradeoff | tui-test is the clear leader (lazy handles, occurrence algebra, style/link selectors, `wait_hidden`); termlens trades handles for immediate queries + masking; tuiscotti's `BoundLocator` is minimal (visible/click, no occurrence algebra) but still exceeds termctrl, which has no selector concept at all. |

## state (out-of-band terminal facts)

| | tuiscotti (executed) | tui-test | termlens | terminal-control |
|---|---|---|---|---|
| Rust | `observe_now() → Observation` (screen + revision + capture reason + `TermState` + provenance); `revision()`, `poll_exit()`, `proto::screen_text` | `Operation::State → State` (shell, size, cursor, title, cwd, last command/exit, modes, mouse, colors) (`api.rs:631,1023`); 12-field `get` | `cursor*/links/title/alternate_screen/bracketed_paste/mouse_mode(s)/clipboard/repaints/bells/graphics/unsupported/frame_timings()` (`screen.rs:852-1166`); query responder names unanswered probes in timeouts (`terminal.rs:1362`) | `SessionStatus{state,exit,cols,rows,idle_for_ms,has_visible_content,recording,…}` (`session.rs:102-116`); `status()`, `logs()`, `semantic_snapshot()` |
| CLI | n/a | `get`, `state` | n/a | `status/list/logs --json/--ansi`, `markers` |
| tradeoff | tui-test adds shell-level facts (cwd, last command/exit) via shell integration; termlens has the richest grid-adjacent state (graphics, clipboard, repaints, bells, unsupported seqs); termctrl is status-centric (fleet view: `idle_for_ms`, retained exited screens). tuiscotti's `Observation` is grid+cursor+palette+modes at one revision — no shell facts, no retained screens. |

## snapshots (cell-exact gates + screenshots)

| | tuiscotti (executed) | tui-test | termlens | terminal-control |
|---|---|---|---|---|
| Rust | `Store::check` (8 statuses, `#[must_use]` outcome) + `accept`; `GroupedStore::accept_all`; `assert_snapshot!` (canonical text); `assert_screenshot!` (canonical + generation-tagged PNG, one sample; mixed generations fail via `check_consistent`) | bespoke `.snap` under `__snapshots__/` — NOT insta (no insta dep): `src/assert/snapshot.rs:11,17,27`; `Passed/Written/Updated` (`api.rs:1126`) | `Display` text format (`screen.rs:2349`) + styled block (`:2285`); `Screen::parse` round-trip; `Screen::diff` + `ScreenDiff` Display; `snapshot_after` returns the settled `Screen`; frozen compat fixtures under `crates/termlens/tests/compat/0.11.x/` | `Shot{frame, ansi}` (`shot.rs:62-67`); live `capture(settle,deadline) → CaptureResult{shot,reason}` with `Idle/Deadline/Exited/OutputClosed` (`session.rs:490-527`); time-travel `recording::shot_at(path, at_ms, marker)` |
| CLI | `render --format png/svg/…` (PNG via swash + `image`; offline re-render byte-identical, exit-0 `diff` proven); `accept/review/report/import` approval lifecycle | `expect snapshot -u --include-style --include-title` (`cli.rs:1595`); `screenshot` to SVG-or-PNG by extension (`--zoom/--background/--transparent/--full`); `record start/stop` APNG/GIF/MP4/cast | `diff` (exit 1, cell-level paint); `render --svg\|--html\|--ansi\|--text\|--json`; NO raster output anywhere (vector/text only by design) | `save --format --out` (PNG via resvg, `render.rs:34-123`); `show` rejects PNG; `video` MP4 export with markers/edit plans |
| tradeoff | tuiscotti is PNG-gated (decoded pixels) with a fail-closed accept/frozen lifecycle and no ambient bless; tui-test updates in place (`-u`) outside the insta ecosystem; termlens is deliberately raster-free (nothing to pixel-gate); termctrl captures (`CaptureReason` names WHY) and time-travels but never asserts. |

## formats (text-side exports + recordings)

| | tuiscotti (executed) | tui-test | termlens | terminal-control |
|---|---|---|---|---|
| Rust | six-format `CaptureBundle` (ASCII 7-bit loss-accounted / TXT / ANSI / PNG / HTML / canonical JSON, one `Generation`) + `cast_v2`/`gif`/`apng` exports; `--format txt\|ansi\|json\|svg\|html\|png` | human text default, `--json` envelope; recordings APNG/GIF/MP4/asciicast-v2 by extension + always-on `.cast` | text, styles block, ANSI, SVG, HTML, JSON (serde round-trip), asciicast v2, diff rendering; failure artifacts `<test>-<n>.screen.{json,txt}` | `ShotFormat{Png,Svg,Txt,Json,Ansi,Semantic}`; `.termctrl` JSONL recordings + `video` MP4 export with markers/edit plans |
| CLI | `inspect` (offline manifest view), `trace --input [--kind]`, `report` (standalone HTML) | `agent-context` schema (`cli.rs:452`); `get-recording`, `monitor`, `usage`, `skill` | `inspect/diff/render` only | `logs`, `markers`, `video`, `driver`, `mcp` |
| tradeoff | tui-test (GIF/MP4) and termctrl (MP4 edit plans) exceed tuiscotti on video; termlens has the broadest text-side matrix incl. JSON round-trip; termctrl's `Semantic` side-channel (app-provided UI JSON) asserts structure, not pixels — unique. tuiscotti's HTML expected/actual/diff report has no equivalent elsewhere. |

## Insta workflow

| | tuiscotti (executed) | tui-test | termlens | terminal-control |
|---|---|---|---|---|
| Rust | `assert_snapshot!` / `assert_screenshot!` (`insta/src/assert/macros.rs`); `pub use insta` re-export; caller-fixed metadata (`source:` + `assertion_line:`); `Policy::{Evolving,EvolvingIn,Frozen}`; evidence on disk BEFORE failure | NONE: no `insta` in manifests/code; bespoke `-u/--update` flow | `insta` DEFAULT feature + re-export (`Cargo.toml`, `lib.rs:175-177`); `assert_screen_snapshot!` settles 100 ms, styles on, one instant (`lib.rs:227-258`); `insta 1.48 + json` dev-dep; `.snap` under `crates/termlens/tests/snapshots/`; PR template mandates `cargo insta review` | ABSENT: no `insta`, no snapshot-assert APIs; review = artifact files + JSONL + `video` export |
| CLI | `cargo insta review` (native); `INSTA_UPDATE=no` stays fail-closed | n/a | `cargo insta review` (native) | n/a |
| tradeoff | tuiscotti and termlens are both native-insta; termlens's macro auto-settles (100 ms) while tuiscotti asserts the caller-supplied settled screen (settle discipline stays at the call site). tui-test/termctrl live outside the Rust insta ecosystem entirely. |

## nextest

| | tuiscotti (executed) | tui-test | termlens | terminal-control |
|---|---|---|---|---|
| Rust | `runner::{TestContext,BaselineId,AttemptId,Journal,ScenarioManifest,JunitKey}`: nextest-aware identity, attempt-safe dirs, journals, manifests | none (only hit is a `brew install` line in README) | none: CI is `cargo test --workspace --all-features` | absent: `cargo test --all-targets` + Vitest |
| CLI | `init` scaffolds `.config/nextest.toml` (scheduling only; never parsed by tuiscotti) | n/a | n/a | n/a |
| tradeoff | tuiscotti is the only one with first-class nextest config + runner-neutral identity; the other three never acknowledge nextest. |

## lifecycle (teardown + sessions)

| | tuiscotti (executed) | tui-test | termlens | terminal-control |
|---|---|---|---|---|
| Rust | `finish(deadline)` graceful / `close()` forceful, idempotent; `Drop` reaps + joins; `PTY_LIFECYCLE` guard; `JOIN_GRACE` 5 s, `KILL_GRACE` 2 s | registry `close/close_all` (`runtime.rs:687,695`); idle watchdog 4 h; Rust has no teardown helpers — the safety net lives in JS/Python glue (`beforeExit`/`trackTerminal`/`closeAllTracked`, `bindings/js/src/test/index.ts:59,64,73`; `terminal()` ctx manager, `bindings/python/src/tui_test/testing.py:201`) | `Drop` kills + bounded-reaps, never joins reader (`terminal.rs:4222-4266`); unix-only `signal()` (`:3952`); `wait_exit` drains with grace; `resize` (`:4105`); macOS `revoke()` analogue absent | daemon per named session: `start_locked` (`session.rs:1230-1310`), `flock` (`:1166`), 0700 runtime dir, `serve()` loop; `stop` SIGKILLs the process group; `restart` from stored `SessionLaunch`; `prune` exited/stale; exited sessions retain final screens |
| CLI | `session {start/stop/list/prune/attach}` (versioned endpoints, owner-only dir, no daemon) | `open/run/restart/close/sessions`, `start/stop`, `internal-daemon` | none | `start/stop/restart/list/prune`, `mark/markers`, `logs` |
| tradeoff | tui-test's Rust core leaks lifecycle to JS/Python glue; tuiscotti and termlens are self-contained in Rust (kill-on-drop, bounded reap, no reader join) with tuiscotti adding graceful `finish` + `CancelToken`. termctrl's daemon persistence (restart/prune/retained screens) is the most operationally complete — and the heaviest. |

## diagnostics

| | tuiscotti (executed) | tui-test | termlens | terminal-control |
|---|---|---|---|---|
| Rust | timeouts fail WITH the screen/observation; `proto::screen_text`; journals + offline verdicts (`Recorder`, `read_journal`) | embedded Svelte trace viewer (prebuilt bundle, `crates/tui-test/assets/trace-viewer/report.*`); per-op diagnostics with locator stages, screen history, hints; session log + PTY tap `Logger` | `Error::{Timeout,Eof,…}` embed + print the screen, `screen()` accessor, `TERMLENS_ARTIFACT_DIR` hook (`error.rs:33-180`); unanswered probes named in timeouts; `unsupported()` seqs | `anyhow` contextual errors; `logs` (10k-row scrollback vs `--ansi` exact bytes); `status/list --json`; video edit-plan schemas |
| CLI | `doctor` (toolchain/fonts/profile/platform/env), `inspect` (offline, never executes), `diff` (exit 4), `review`/`report` | richest failure artifacts: HTML and/or MD + manifest + optional recording | `TERMLENS_ARTIFACT_DIR` CI hook (no CLI surface) | fleet introspection over the daemon |
| tradeoff | tui-test diagnostics are the richest (trace viewer + per-op locator-stage timelines); termlens's artifact-dir hook is the CI-friendly gap tuiscotti lacks; tuiscotti covers toolchain/offline/diff/review with no trace UI. |

## safety (`unsafe` surface)

| | tuiscotti (executed) | tui-test | termlens | terminal-control |
|---|---|---|---|---|
| Rust | `unsafe_code = "forbid"` workspace-wide; ZERO `unsafe` blocks (verified: every `unsafe` hit in the tree is a comment or a `forbid` attribute) | core: exactly ONE block — `MoveFileExW` atomic replace on Windows (`src/session.rs:772`); CLI: `GetStdHandle`/console, `SetHandleInformation`, `monitor.rs` Windows; napi glue; pyo3 clean; none in emulator/locator/snapshot | 3 audited lib blocks: `dup(2)` (`terminal.rs:336`), `from_raw_fd` (`:341`), `kill` (`:3952`), each `#[allow(unsafe_code)]`; workspace `unsafe_code = "warn"`; counted in `SECURITY.md:13`; test-only `GlobalAlloc` + fixture ioctl | ~30 sites, all FFI/test, none in parse/render: SIGKILL pg, flock, setsid `pre_exec`, geteuid, poll, unix socket/fcntl |
| CLI | n/a | n/a | n/a | n/a |
| tradeoff | tuiscotti `forbid` stands alone (strictest); termlens justifies each block in `SECURITY.md`; tui-test confines `unsafe` to Windows FFI + napi glue; termctrl's daemon machinery needs the widest surface — but none of the four has `unsafe` in frame/render logic. |

## extensibility (protocols, bindings, features)

| | tuiscotti (executed) | tui-test | termlens | terminal-control |
|---|---|---|---|---|
| Rust | crate split (core/render/runtime/insta/cli/fixtures); `mcp` module (tools + stdio serve); `machine` typed op protocol over stdio (15 ops, protocol 2.0.0, no daemon); `tui_shell`, `grouped`, `import_compat` | napi JS module + TS client/sessions/ephemeral (`bindings/js/`); pyo3 module + client/testing/diagnostics (`bindings/python/`); `pub trait Emulator: Send` (`terminal/emu.rs:411`) | cargo features only, NO plugin API: `insta` (default), `decode` (miniz_oxide bitmaps), `regex` (matches/masks), `serde` (Screen JSON round-trip); MSRV 1.85 | MCP server `TerminalControl`, stdio `serve()` (rmcp), 9 tools; JSON schemas `frame-v{1,2}`, `recording-entry-v{1,2}`, `video-edit-v1`; driver JSONL stdio `PROTOCOL_VERSION=2` (13 methods); TS client + Vitest adapter; OpenTUI semantic provider v6 |
| CLI | `schema` (prints the op-protocol JSON Schema); `machine` (op JSON in, envelopes out) | `agent-context`, `skill`, root `SKILL.md` | none | `driver`, `mcp`, `__serve` (hidden) |
| tradeoff | tui-test is the only one with production JS+Python bindings; termctrl has the richest machine surface (MCP + driver protocol + schemas + TS/Vitest + semantic side-channel) — tuiscotti's `mcp` + `machine` is a deliberate subset (Rust-only transport, no daemon). termlens extends via features alone. |

## Honest strengths (what each competitor does that tuiscotti does not)

**tui-test** — (a) the only competitor with true Playwright-style
lazy locators + expect/wait vocabulary in Rust; (b) 4 emulator
backends for conformance testing; (c) PNG *and* SVG screenshots
plus MP4/GIF/cast recording in one tool; (d) daemon + session
multiplexing + JS/Python bindings = multi-agent/multi-language
story; (e) richest structured failure diagnostics (trace viewer,
HTML+markdown reports, trace bundles); (f) shell-integration waits
(`command`/`ready`).

**termlens** — (a) deepest terminal-fidelity model: sync-frame
waits (no torn repaints), NFC matching, wide-char/scrollback/
insert-mode handling, graphics-protocol counting, query answering;
(b) out-of-band assertions others miss: cursor shape, OSC-52
clipboard, OSC-8 links, repaints, bells; (c) best snapshot
ergonomics: one-macro settle+styles+snapshot with insta, plus
diff/mask/parse/serde round-trips; (d) most disciplined test corpus
(34 integration files + fixtures + compat evidence + semver gates);
(e) timeout errors carry the screen; (f) lightest dependency tree
(5 required crates, MSRV 1.85).

**terminal-control** — (a) `Shot` keeps `frame` + source `ansi`
together; (b) time-travel: `shot_at(recording, at_ms|marker)` +
`markers` + video export from one file; (c) unique **semantic
snapshot** channel (app-provided UI JSON via socket) — asserts
structure, not pixels; (d) MCP server + external driver protocol =
agent-native control plane; (e) single-crate simplicity with
copy-paste Rust examples; (f) daemon lifecycle done carefully
(flock-guarded owner, uid-checked 0700 dir, process-group kill,
retained exited screens).

**Where tuiscotti differs** — explicit approval workflow with no
ambient bless (fail-closed stores, generation-bound compound
gates); pinned render profile with vendored-font determinism
(byte-identical PNGs, executed); typed op protocol + MCP bridge
without a daemon; pure-view path with zero subprocess (executed
with `--no-default-features`); first-class nextest support; and
the strictest safety posture (`unsafe_code = "forbid"`, zero
blocks).

## Cross-cutting summary

| Capability | tuiscotti | tui-test | terminal-control | termlens |
|---|---|---|---|---|
| PTY spawn + key/mouse input | yes (chords, ModeNotEnabled) | yes (tokens, SGR) | PTY yes; paced bytes; mouse w/ reporting check | yes (Key/Chord, SGR) |
| Waits with evidence on timeout | yes | yes | yes (no assert glue) | yes (screen in error) |
| Cell-exact snapshot gates | yes (cells + ANSI) | yes (bespoke, `-u`) | no (capture only) | yes (insta macro + diff) |
| PNG pixel gates | yes (decoded-pixel) | yes | yes (resvg) | **no (by design)** |
| SVG/HTML/text/ANSI export | yes | yes | yes | yes (no PNG) |
| Video/cast recording | cast/GIF/APNG export | APNG/GIF/MP4/cast | .termctrl + video | asciicast |
| Explicit approval lifecycle | yes (accept/frozen/review) | in-place `-u` | no | insta review |
| Agent control plane | op protocol + MCP, no daemon | daemon + skill + bindings | MCP + driver protocol | none |
| Test corpus at pin | 519 tests, 8-example lane (executed) | runtime + CLI suites | thin (daemon + unit) | 34 integration files |
