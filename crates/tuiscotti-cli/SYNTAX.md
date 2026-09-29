# tuisnap syntax matrix (G6)

One place for every Rust and CLI surface: setup, render, launch, waits,
input, locators, capture, assertions, profiles, exports, frozen review,
machine ops, cleanup. Rust paths are `tuiscotti::…` unless noted.

## Setup

| Task | Rust | CLI |
|---|---|---|
| Scaffold project | — | `tuisnap init --dir . [--force]` |
| Toolchain/fonts/profile/env report | — | `tuisnap doctor` |
| Op-protocol schema | `proto::PROTOCOL_SCHEMA_JSON` | `tuisnap schema` |
| Attempt-safe test dirs | `runner::TestContext::current(s)` | — |

## Pure render (no PTY, no subprocess)

| Task | Rust |
|---|---|
| Draw closure → `Screen` (strict clips) | `ratatui::render((100, 30), \|f\| …)?` |
| Draw closure → capture + clip record | `ratatui::render_screen(c, r, draw, EdgePolicy)` |
| Widget / stateful widget | `ratatui::widget_screen / stateful_screen` |
| Raw buffer / test backend | `ratatui::screen_from_buffer / screen_from_test_backend` |
| Lenient vs strict row-end clips | `EdgePolicy::ClipWithReplacement` (default) / `::Error` |

## Piped launch (`Command`: one program + args, `OsStr`/`Path` natives)

| Task | Rust | CLI |
|---|---|---|
| Build | `Command::new(p)` / `Command::cargo_bin(n)` | — |
| Args / env / cwd (child-only) | `.arg/.args/.env/.envs/.env_remove/.env_clear/.current_dir` | `capture --out D -- prog args…` |
| stdin / timeout / limits | `.stdin(bytes) / .timeout(d) / .output_limit(n) / .drain_deadline(d)` | `--timeout-ms` |
| Shell opt-in | `.shell(true)` (`/bin/sh -c`) | — |
| Isolated HOME/XDG/cwd fixture | `command::isolated_env()` + `IsolatedEnv::apply` | — |
| `std` interop | `Command::from_std / std_command` | — |
| Run → truthful output | `.run()` → `ProcessOutput` | `record --out T -- prog…` |
| Termination (never conflated) | `Termination::Exit / Signal / Timeout / OutputLimit / SpawnError` | manifest `termination` |
| Typed spawn detail | `ProcessOutput::error: Option<SpawnError>` (`kind/detail/searched`) | — |
| UTF-8 views (fallible first) | `stdout_str / stderr_str`; lossy only via `stdout_lossy / stderr_lossy` | — |
| Incomplete drain | `ProcessOutput::truncated` | manifest `truncated` |

## Live launch (`Tui`, feature `pty`)

| Task | Rust |
|---|---|
| Build | `Tui::new(argv)` / `Tui::cargo_bin(name)?` (eager, typed error) |
| Args / size / child env/cwd | `.arg/.args/.size(c,r) / .env(k,v) / .cwd(dir)` |
| Terminal behavior | `.profile(TerminalProfile)` (unsupported claims rejected) |
| Spawn | `.spawn()?` → `Session` (`Send + Sync`) |
| Binary lookup | `CARGO_BIN_EXE_<name>` → next to test exe (`target/<profile>/`) |

## Waits (simple `Duration` / advanced deadline + token)

| Task | Simple (`Duration`, internal cancel) | Advanced (`deadline: Instant`, `&CancelToken`) |
|---|---|---|
| Immediate observation | `observe_now()` | — (is the primitive) |
| Predicate | `wait_predicate_timeout(f, d)` | `wait_predicate(f, deadline, cancel)` |
| Output settled | `wait_stable_timeout(d)` (`DEFAULT_WAIT` = 10s) | `wait_stable(_quiet)` |
| Synchronized frame | `wait_frame_timeout(d)` (fails closed: `Unsupported`) | `wait_frame(deadline, cancel)` |
| Child exit + evidence | `wait_exit_timeout / expect_exit_timeout` → `ExitWait` | `wait_exit / expect_exit` |
| Exit assertions | `ExitWait::success() / .code(n)` → `Observation` | same |
| Negative temporal | `Locator::eventually_absent / remains_absent / expect_count` (detached, `Duration`) | — |
| Failure shape | `WaitError::{Timeout, Cancelled, Unsupported, Closed}` — always with evidence | same |

## Input

| Task | Rust |
|---|---|
| Text / raw bytes / paste | `send_text / send_bytes / paste` (bracketed when negotiated) |
| Chords | `press("Ctrl+P")`; typed `press_key / key_down / key_repeat / key_up / key_event` |
| Typed keys | `Key`, `KeyMods`, `KeyChord: FromStr`, `Key: FromStr` (bare names) |
| Mouse / focus / resize / signals | `click / mouse_down / mouse_up / mouse_move / mouse_drag / mouse_wheel`, `focus_in/out`, `resize`, `signal` |

## Locators

| Task | Rust |
|---|---|
| Session-bound (fresh obs each call) | `session.get_by_text(t) / .get_by(loc)` → `BoundLocator` |
| Bound actions | `.expect_visible() → Span`; `.click()` (unique + stale-checked, at most once) |
| Action errors (typed, sourced) | `ActionError::{Session(TuiError), Locate(LocateError)}` |
| Detached queries (advanced/offline) | `Locator::text / regex / style / region`, `.mode/.within/.before/.after/.nth/.first/.last/.and/.or/.filter` |
| Detached resolve | `.resolve / .resolve_unique / .resolve_obs` (+ `…_with_scrollback`) |
| Readiness without sinks | `Locator::prepare_action / prepare_action_retry` → `PendingAction::click / submit` |

## Capture

| Task | Rust |
|---|---|
| Grid snapshot | `session.snapshot() → Screen`; `observe_now() → Observation` |
| Screen model | `Screen` (validated, invariant-preserving) + `TryFrom<&Frame>` |
| Sub-regions | `screen.region(x, y, c, r, RegionPolicy)` (never splits wide glyphs) |
| Renderers take `&Screen` | `Renderer::render_screen / render_screen_png`, `render::render_screen` |

## Assertions (Insta-native, caller-fixed metadata)

| Task | Rust |
|---|---|
| Canonical state | `tuiscotti::assert_snapshot!(name, &screen[, &policy])` |
| Canonical + PNG, one sample | `tuiscotti::assert_screenshot!(name, &screen[, &policy])` (PNG = `<name>-img`) |
| Policies | `Policy::{Evolving, EvolvingIn{snapshots, evidence}, Frozen{root}}` |
| Metadata | `source:` + `assertion_line:` name the caller; description carries generation + `render <profile>/rv<N>/<alpha>` + caller |
| Compound gate | `check_consistent` (strict) / lenient inside `assert_screenshot!` |
| Same-sample evidence | `<name>.{png,ansi,txt,html}` written BEFORE any failure |

## Profile customization (no per-test manual setup)

| Task | Rust |
|---|---|
| Session terminal | `TerminalProfile` fields (mouse/paste/focus/altscreen/kitty) |
| Raster profile | `Profile::default_profile()` + `Renderer::new(&profile, &faces)` |
| Strict render pins | `RenderProfile` (geometry/scale/palette/cursor/blink/missing/renderer version) |
| PNG comparison | `PngPixelComparator::new(alpha)` / `png_comparator(alpha)`; `AlphaPolicy::{StraightRgba, Opaque}` |
| Error umbrella | `tuiscotti::{Error, Result}` (`?` from every typed error, sources kept) |

## Exports (all formats)

| Format | Rust | CLI |
|---|---|---|
| TXT (plain Unicode) | `Renderer` artifacts / `frame.text()` | `--format txt` |
| ANSI (normalized SGR) | `render::ansi_dump` | `--format ansi` |
| Canonical JSON | `frame.to_json()` | `--format json` |
| SVG (static) | `render::render_svg` | `--format svg` |
| HTML (offline, no JS) | `Renderer::render_html` | `--format html` |
| PNG (+ fidelity sidecar) | `Renderer::render_png / render` | `--format png` |
| Four-artifact bundle | `assert::emit_four(&screen, dir)` | `render --input F --format … --out P [--font-file F]` |
| Compare / inspect | `diff::compare_png…` | `diff --expected A --actual B`; `inspect --dir D`; `trace --input J [--kind K]` |

## Frozen review (immutable references, never self-heal)

| Task | Rust | CLI |
|---|---|---|
| Check against frozen root | `assert::check_frozen_snapshot / check_frozen_screenshot` | `import --dir D` (read-only tree view) |
| Frozen asserts | `assert_frozen_snapshot / assert_frozen_screenshot` (+ `Policy::Frozen`) | — |
| Accept (evolving only) | `snapshot::Store::accept` / `GroupedStore` | `accept --store S name` (frozen roots reject) |
| Verdicts / report | `proto::read_verdicts / write_html_report` | `review --dir D`; `report --dir D --out R [--title T]` |

## Machine ops

| Task | CLI / Rust |
|---|---|
| Op protocol over stdio | `tuisnap machine < ops.jsonl` (envelope JSON per line; exit 0/3) |
| Typed ops (spawn/observe/input/assert/…) | `proto::Op`, `proto::execute`, `proto::run_machine_line` |
| Named sessions | `session {start --name N [--force] -- argv… \| stop \| list \| prune \| attach --name N}` |
| Session API | `proto::session_start_os (OsString) / session_start / session_stop / session_list / session_prune` |

## Cleanup

| Task | Rust / CLI |
|---|---|
| Graceful / forceful teardown | `session.finish(deadline)` / `session.close()` (`Drop` reaps + joins, never double-panics) |
| Session teardown | `session stop --name N`; `session prune`; liveness via `tui::process_exists` |
| Attempt-safe evidence | `runner::{TestContext, Journal}` (no global env/CWD mutation anywhere) |

## Exit policy

`0` ok · `2` CLI usage error · `3` tool/op error · `4` verification
disagreement (`diff` mismatch, `review` failures). `capture`/`record`
preserve the CHILD's exit code. Failing assertions always leave evidence
(png/ansi/txt/html, journals, `.snap.new`) before failing.
