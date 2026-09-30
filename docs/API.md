# Public Rust API design

Facade: `tuiscotti` re-exports the leaf modules; paths below are the
facade paths. Status labels: **implemented** (shipped, tested),
**partial** (works with documented gaps), **unsupported**
(explicitly rejected, fails closed), **future** (planned, absent).

## Capture

| Item | Status | Notes |
|---|---|---|
| `ratatui::{draw_frame, widget_frame, capture, render_screen, widget_screen, stateful_screen}` | implemented | Production draw closures → `Frame`/`Screen`. No PTY. |
| `tui::{Tui, Session}` (`Tui::new/cargo_bin/size/env/cwd/profile/spawn`) | implemented | Owned PTY sessions, feature `pty`. |
| `tui::Session::{snapshot, observe_now, wait_predicate, wait_stable, wait_stable_quiet, wait_frame, wait_exit, expect_exit, finish, close}` | implemented | Every wait fails with evidence on timeout; `close` reaps. |
| `tui::Session::{press, press_key, key_down, key_repeat, key_up, key_event}` + free fn `tui::parse_chord` | implemented | `+`-joined chords (`ctrl+Up`); key down/repeat/up distinct. |
| `tui::Session::{send_text, send_bytes, paste}` | implemented | Literal input; `paste` is bracketed paste. |
| `tui::Session::{click, mouse_down, mouse_up, mouse_move, mouse_drag, mouse_wheel, focus_in, focus_out}` | implemented | Fail with `ModeNotEnabled` unless the app enabled reporting. |
| `tui::Session::{resize, signal, pid, revision, poll_exit}` | implemented | Resize reflows via the emulator; signals unix-only. |
| `tui::Session::meta` → `tui::SessionMeta` | implemented | Cheap revision + geometry under one lock — no worker round trip, no screen clone (F12). |
| `tui::Session::{get_by, get_by_text}` → `BoundLocator::{visible_now, expect_visible[_within], click}` | implemented | Immediate lookup vs bounded retrying expectation (10s default); click resolves + delivers atomically in the owning worker (F11). |
| `tui_shell::{Recording, Replayed, replay_*, TermSnapshot, assert_*}` | implemented | Recording/replay (`replay_bytes`/`replay_chunks`/`replay_recording` → `Replayed`) + terminal-state assertions (title, modes, palette, clipboard, links, scrollback). |
| `command::{Command, ProcessOutput, IsolatedEnv}` | implemented | Piped child runs: timeouts, output limits, split streams,exit/signal distinction (`Termination`). |

## Query

| Item | Status | Notes |
|---|---|---|
| `locate::Locator::{text, regex, regex_case_insensitive, style, region}` | implemented | Playwright-style; `StyleQuery` covers fg/bg/bold/dim/italic/underline(+style/color)/strike/reverse/hidden/blink/custom. |
| `Locator::{within, before, after, nth, first, last, and, or, filter, mode, physical_rows}` | implemented | Combinators over spans. |
| `Locator::{resolve, resolve_unique, expect_visible, expect_text, expect_count, present_now, not_present_now, eventually_absent, remains_absent, prepare_action}` | implemented | Waits take one deadline; `PendingAction::{click, submit}` acts at a resolved span. |
| `semant::{by_role, Role, SemNode, HitRegion}` | implemented | App-provided semantic trees; `center()` gives click points. |
| `observe::{screen_text, compare_replay_vs_rerun, Watcher, Replay, Rerun}` | implemented | `Watcher` (feature `pty`) polls a session; `Replay`/`Rerun` compare recorded vs re-executed output. |

## Gates

| Item | Status | Notes |
|---|---|---|
| `snapshot::{Store, Status, CompareOutcome}` | implemented | Classic store; 8 statuses; `CompareOutcome` is `#[must_use]`. |
| `grouped::{GroupedStore, GroupedOutcome, ArtifactPaths}` | implemented | Nested names; exactly 4 committed artifacts; `accept_all`. |
| `assert_snapshot!` / `assert_screenshot!` | implemented | Insta-backed; canonical text gate / canonical+PNG compound gate with generation binding. |
| `assert::{render_sample, generation_id, png_tag_generation, png_generation, check_consistent}` | implemented | Compound lifecycle: mixed generations fail, never half-pass. |
| `assert::{check_frozen_snapshot, check_frozen_screenshot, frozen_accept}` | implemented | Frozen policy: read-only, never self-heals; accept always errors. |
| `assert::{emit_four, import_frozen_v1}` | implemented | 4-artifact export from one generation; read-only frozen import. |
| Ambient bless (`BLESS=1`, `UPDATE_SNAPSHOT=1`) | unsupported | Removed; a test proves no env var accepts. Explicit accept only. |

## Rendering and diff

| Item | Status | Notes |
|---|---|---|
| `profile::{Profile::default_profile, RenderProfile::strict/vendored, FontFaces, FallbackFace}` | implemented | Pinned: font SHA-256, 10×21 cells @16px, scale ×2. |
| `render::{Renderer, render_png, render_svg, ansi_dump, frame_from_screen, redact_frame}` | implemented | PNG/SVG/ANSI/HTML + fidelity sidecars; `Renderer::with_fallbacks` for custom chains (pins verified at load). |
| `render::Renderer::{with_profile, with_strict}` | implemented | Thread-local shared renderers (F12): faces parsed once per thread, glyph caches shared; custom profiles construct per call. |
| `render::{CacheOptions, RenderCache::open_with_options}` | implemented | No-cache mode is a per-cache context (F12) — no process-global state; OR-ed with `RENDER_NO_CACHE`. |
| `screen::{canonical_string, canonical_value}` | implemented | Deterministic state projections every gate binds (F12: moved from the insta spike into core). |
| `formats::{capture_all, CaptureBundle, Generation, pipe_projection, …}` | implemented | Six-format contracts (ASCII/TXT/ANSI/PNG/HTML/canonical JSON) + piped-byte projections with generation binding. |
| `diff::{compare_png, compare_png_with_alpha, perceptual_score, PerceptualPolicy}` | implemented | Exact decoded-pixel gate; `PerceptualPolicy::new` rejects NaN/out-of-range thresholds. |
| `export::{cast_v2, gif, apng}` | implemented | asciicast/GIF/APNG evidence exports. |

## Agents and CI

| Item | Status | Notes |
|---|---|---|
| `proto::{Op, OpResult, OpError, execute, run_machine_line}` | implemented | 15 typed ops (spawn/stdin/observe/snapshot/screenshot/wait/exit/assert/render/diff/session-start/session-stop/session-list/version/capabilities — no prune op); JSON envelopes over stdio. |
| `proto::{session_start, session_stop, session_list, session_prune, runtime_dir}` | implemented | Named sessions, versioned endpoints, owner-only runtime dir. |
| `proto::{Recorder, read_journal, Verdict}` | implemented | Bounded event journals + offline verdicts. |
| `proto::LogTail` | implemented | Bounded incremental session-log tail (F12): per-poll + lifetime caps, truncation flagged, never re-reads whole. |
| `proto::set_runtime_dir_override` | test-only (`test-overrides`) | Exists only under `cfg(test)` or the `test-overrides` feature; production resolves the runtime dir from the process env (F12). |
| `mcp::{tools, tools_list_json, serve, run_stdio, handle_request}` | implemented | MCP stdio bridge over the op protocol. |
| `runner::{TestContext, BaselineId, AttemptId, Journal, ScenarioManifest, JunitKey}` | implemented | Runner-neutral identity (nextest-aware), evidence dirs, journals, scenario manifests. |

## Conventions

- Waits fail, never return false: timeouts carry the last
  observation/screen so CI failures are debuggable without artifacts.
- `#[must_use]` on outcomes (`CompareOutcome`, `GroupedOutcome`,
  `Status`): dropping a gate without asserting warns instead of
  silently passing.
- Errors are typed per module (`FrameError`, `TuiError`,
  `SnapshotError`, …); the op layer maps them to `OpError`
  `{code, message, session?}` envelopes.
- `redact_frame`/`redact_screen` exist for scrubbing captures, but
  hidden text is conceal, not redaction — never capture real secrets.
