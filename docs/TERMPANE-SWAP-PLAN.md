# G1 termpane backend swap — assessment + durable plan

Branch: `redesign/rust-first-testing-platform` (revalidated at `68b4967`).
Upstream reference: `tailrocks/termpane` `main` at `dc40286`
(`dc40286f9a8942f19f3a4cdebaaf1e18b7321709`), the squash-merge of
PR #25 (`feat/process-pty-transport`), checked out read-only at
`/tmp/termpane-qual`. Supersedes the PR-branch assessment pinned to
`dad8389` (that object is not in the qual clone — squash merge — so
dad8389→dc40286 drift below is inferred from the old pins vs merged
source; see Appendix A).

Target end state: **no direct dependency** on `portable-pty`,
`alacritty_terminal`, or `libc` anywhere in the workspace; the PTY
backend comes only from a **released** `termpane` from crates.io.
(`libc`/`portable-pty` remain in the graph *transitively* regardless —
see §2. Transitive is permitted; direct is banned.)

## 0. External blocker — the swap cannot land yet

Two stacked facts, re-verified 2026-09-30:

1. **PR #25 is merged** (`dc40286`, `main`). The `process` / `pty` /
   `session` modules are on `main`; the merge half of the old blocker
   is cleared.
2. **`termpane` still has zero crates.io releases.**
   `https://crates.io/api/v1/crates/termpane` (with UA header; bare
   curl gets a 403) and the sparse index
   `https://index.crates.io/te/rm/termpane` return **404**. There is
   no published version to pin. `Cargo.toml` on `main` says
   `version = "0.1.0"`, so the release to watch for is presumably
   `0.1.0` (re-run §1.5/§2 against the actual release — the map is
   pinned to `dc40286`, not to "0.1.0, whatever it contains").

Plus a permission fact: this project cannot publish `termpane`
itself (upstream `tailrocks` owner must release). Until a registry
release containing the §1.5 API surface exists, the swap is
**externally blocked**. A git or path dependency is not an acceptable
bridge (§7). This document is evidence + plan, not the swap.

## 1. Inventory: every occurrence of the three crates

Re-inventoried at `68b4967` via
`rg -n 'portable_pty|alacritty_terminal|use libc|libc::' crates`
(65 hits). Net change since `f89d522`: `spawn.rs` split out of `builder.rs`
(`6c5b559`: +6 hits, `builder.rs` 4→2), `session_input.rs` comment gone
(signals route through the worker). No new holders.

### 1.1 Direct manifest dependencies

Exactly one holder, both seams documented as temporary:

| Manifest | Occurrence |
|---|---|
| `Cargo.toml` `[workspace.dependencies]` | L32 comment + `portable-pty = "=0.9.0"` (L33), `alacritty_terminal = "=0.26.0"` (L34), `libc = "=0.2.189"` (L35) |
| `crates/tuiscotti-runtime/Cargo.toml` | `pty = ["dep:portable-pty", "dep:alacritty_terminal", "dep:libc"]` (L14); three `optional = true` deps (L28–30) |

No other workspace manifest mentions any of the three — including no
`[dev-dependencies]`, `[build-dependencies]`, or `[target.*]` sections
for them anywhere (`rg '\[target' crates/*/Cargo.toml Cargo.toml`
returns nothing; `tuiscotti-runtime` has no dev/build sections at all).
Feature wiring above the runtime is pure forwarding with
`default-features = false` on the path deps:

- `tuiscotti`: `pty = ["tuiscotti-runtime/pty"]`, default on.
- `tuiscotti-cli`: `pty = ["tuiscotti/pty"]`, default on; binary always
  builds, offline commands work without `pty`.
- `tuiscotti-fixtures`: `pty = ["tuiscotti/pty"]`, default on.

`cargo tree -p tuiscotti-runtime --no-default-features` confirms the
pure-view graph contains none of the three.

### 1.2 First-party source uses (code, not comments)

14 files. All inside `tuiscotti-runtime` (13 under `src/`, 1 test):

**`src/tui/` — live session path**

| File | `portable-pty` uses | `alacritty_terminal` uses | `libc` uses |
|---|---|---|---|
| `tui/builder.rs` | `CommandBuilder` (L9; `prepare_command` L275–296) | `TermConfig` (L8, L238–240) | — |
| `tui/spawn.rs` | `CommandBuilder`, `PtySize`, `native_pty_system` (L12); `Box<dyn MasterPty>` / `Box<dyn Child>` (`SpawnedPty` L22–28); openpty/spawn/handles/poll/pid + rollback (`spawn_pty_child` L34–84, `rollback_child` L90–127) | `TermConfig` (L11, L164) | — |
| `tui/worker.rs` | `Child as PtyChild`, `MasterPty` (L9); `portable_pty::ExitStatus` (L286); `try_wait`/`kill` in `poll_child`/`shutdown_child` (L284–333) | `Event`, `EventListener`, `GridDims`, `TermConfig`, `Term` (L6–8) | — |
| `tui/worker_ctx.rs` | `Child as PtyChild`, `MasterPty` (L13, L46–47, L57–58) | `Event`, `Term`, `Processor` (L10–12); `processor.advance` feed (L146) | — |
| `tui/capture.rs` | — | `Event`, `EventListener`, `WindowSize`, `Term`, `VteRgb` (L5–7); reply/color/size drain (L51–99) | — |
| `tui/encode.rs` | `MasterPty`, `PtySize` (L13, `apply_resize` L144–164) | `EventListener`, `GridDims`, `Term`, `TermMode` (L10–12); pure mode-gated `encode_input` (L127–142); `term.resize` (L159) | — |
| `tui/encode_key.rs` | — | `TermMode` (L3): `KITTY_KEYBOARD_PROTOCOL` (L27), `APP_CURSOR` (L75) | — |
| `tui/exit.rs` | `From<portable_pty::ExitStatus>` (L43–44) | — | — (comment only, L105; `process_exists` L101–114, non-Unix stub L116) |
| `tui/frame.rs` | — | `EventListener`, `index::{Column,Line}`, `Term`, `TermMode`, `CellFlags`, `VteColor`, `CursorShape`, `NamedColor` (L7–12) + fully-qualified `term::cell::Cell` (L143); modes→numbers map (L267–287) | — |
| `tui/input_types.rs` | — | — | `libc::SIGINT/SIGTERM/SIGKILL/SIGQUIT/SIGHUP` (L245–249, fully qualified; no `use libc`). Only direct `libc` code use in the repo |

**`src/tui_shell/` — replay path** (live `Shell`/`Guardian` touch no
banned crate; containment is `ps`/`kill` subprocesses in `unix.rs`)

| File | Uses |
|---|---|
| `tui_shell/replay_api.rs` | `Event`, `EventListener`, `GridDims`, `TermConfig`, `Term`, `Processor` (L3–6); fresh-emulator replay (L212–225) |
| `tui_shell/replay_screen.rs` | `Event`, `EventListener`, `index`, `CellFlags`, `ClipboardType`, `Term`, `TermMode`, `VteColor`, `CursorShape`, `NamedColor` (L3–7) + FQN cell (L92) |
| `tui_shell/replay_state.rs` | `EventListener`, `GridDims`, `index`, `CellFlags`, `Term`, `TermMode`, `NamedColor`, `VteRgb` (L3–8) + FQN grid (L104, L126) |

**Tests + tooling**

| File | Uses |
|---|---|
| `tests/tui_shell/replay.rs` | `use portable_pty::{CommandBuilder, PtySize, native_pty_system}` (L45) in the `capture_raw` helper — compiles only because integration tests link the package's (optional, default-on) deps; gated by `#![cfg(feature = "pty")]` |
| `crates/xtask/src/deps.rs` | Name strings in `BANNED` (L23) + `TEMP_HOLDER` exception (L26, L155–156). Updated by the swap, not deleted |

Comment/doc-only mentions (updated mechanically with the swap):
`tui/mod.rs` (L3, L13), `tui/limits.rs` (L7, L10), `tui_shell/unix.rs`
(L20, L238), `frame.rs` (L162, L177), `replay_screen.rs` (L125),
`tuiscotti-fixtures/.../consumer/main.rs` (L4),
`tuiscotti-fixtures/tests/underline.rs` (L9). (`proto/sessions.rs`,
`proto/session_ops.rs`, and `session_input.rs` no longer mention the
crates — old pins withdrawn.)

### 1.3 Transitive graph (locked)

`Cargo.lock`: `portable-pty 0.9.0`, `alacritty_terminal 0.26.0`,
`libc 0.2.189`, all from the crates.io registry. Reverse edges:

- `portable-pty`: sole user `tuiscotti-runtime`. After the swap it
  re-enters **transitively** via `termpane` (`pty` feature pulls
  `portable-pty 0.9` — `/tmp/termpane-qual/Cargo.toml` L35, L70).
- `alacritty_terminal`: sole user `tuiscotti-runtime`. Leaves the graph
  (nothing else wants it).
- `libc`: **17 users** — direct `tuiscotti-runtime` plus
  `alacritty_terminal`, `cpufeatures`, `errno`, `filedescriptor`,
  `getrandom`, `mio`, `nix`, `parking_lot_core`, `portable-pty`,
  `rustix`, `rustix-openpty`, `serial2`, `shared_library`,
  `signal-hook`, `signal-hook-mio`, `signal-hook-registry`. It stays
  locked via `crossterm`/`tempfile`/`sha2`/… regardless of the swap,
  and `termpane process` adds `nix 0.31 → libc` too. The goal is **no
  direct `libc` dep**, never a libc-free graph.
- `termpane`: absent from the graph.

### 1.4 Evidence commands (re-run to confirm)

```sh
# Direct deps incl. kind/optional/target (tuiscotti-runtime only)
cargo metadata --offline --format-version 1 --no-deps
# Reverse edges (normal + dev + transitive)
cargo tree --offline -i portable-pty@0.9.0 --depth 2
cargo tree --offline -i alacritty_terminal@0.26.0 --depth 2
cargo tree --offline -i libc@0.2.189 --depth 3
# Feature gating proof
cargo tree --offline -p tuiscotti-runtime --no-default-features --depth 1
cargo tree --offline -p tuiscotti-runtime -e features -i portable-pty
# Source uses
rg -n 'portable_pty|alacritty_terminal|libc::|use libc' crates --glob '!**/Cargo.lock'
# Lock reverse-deps: parse Cargo.lock [[package]] dependencies (see §1.3)
```

### 1.5 Superseded backend adapter surface (what owns PTY/process today)

All under `crates/tuiscotti-runtime/src/`, gated by `feature = "pty"`
(`lib.rs` L21–30):

- **Spawn**: `tui/builder.rs` — `Tui` builder (argv/env/cwd/size/
  profile) → `spawn()` (L205); `prepare_command` (L275–296, `TERM`
  default); `tui/spawn.rs` — `spawn_pty_child` (L34–84: openpty +
  spawn + handles + pid under `PTY_LIFECYCLE`, rollback on partial
  failure) + `start_session_threads` (L162–258: writer+worker+reader).
- **Worker loop**: `tui/worker.rs` (`Op` enum, `run_worker` L154,
  `poll_child` L284, `shutdown_child` L296) + `tui/worker_ctx.rs`
  (`WorkerCtx`: owns `Term`, writer, child; feed/eof/observe/input/
  resize/close-input/shutdown/disconnect; exit poll with `DRAIN_GRACE`).
- **Reader/event drain**: `tui/capture.rs` (`run_reader` L19,
  `drain_term_events` L47 — title/bell/reply/clipboard/color/size
  answers, `resolve_color` L109).
- **Observation build**: `tui/frame.rs` (`build_observation` L23: grid +
  cursor + palette + modes at one revision; `push_modes` L267–287).
- **Input encoding**: `tui/encode.rs` + `tui/encode_key.rs` (mode-gated
  bytes from live `TermMode`); `tui/session_input.rs` (`Session` input
  methods; Unix `signal()` L303–328 via `kill(1)`, `Signal::number()`
  is the only `libc` use).
- **Waits/teardown**: `tui/session.rs` (observe/waits incl.
  always-`Unsupported` `wait_frame`), `tui/session_teardown.rs`
  (`finish`/`close`/`Drop`, bounded joins), `tui/shared.rs`
  (revisioned publish + `Condvar`), `tui/exit.rs` (`ExitStatus`,
  `ExitWait`, `process_exists` L101–114 via `kill -0`), `tui/error.rs`
  (`TuiError`, `WaitError`, `CancelToken`).
- **Shell/guardian**: `tui_shell/shell.rs` (OSC 133 marker protocol over
  `Session`), `tui_shell/guardian.rs` + `tui_shell/unix.rs`
  (process-group sweep via `ps`/`kill` — no banned deps).
- **Replay**: `tui_shell/replay_api.rs` (`Recording`, `replay_*`
  L182–247, `MAX_REPLAY_BYTES` L18, chunk invariance) +
  `replay_screen.rs` + `replay_state.rs` (fresh `Term`,
  `REPLAY_HISTORY = 1000`).
- **State assertions**: `tui_shell/state.rs` (`TermSnapshot`,
  `SandboxClipboard`, `assert_*`).
- **Profile gates**: `tui/profile.rs` (2026 rejection L77–82, blink
  rejection L84–86) + `tui/limits.rs` (`MIN_COLS = 2`, timing
  constants, `PTY_LIFECYCLE`).

Public API stability boundary (must not change shape):
`tuiscotti::tui::{Tui, Session, …}` facade re-exports,
`Observation`/`TermState`, `TermSnapshot`, `Recording`/`Replayed`.

## 2. Coverage map: adapter capability → termpane main API

All paths re-verified against `/tmp/termpane-qual/src` at `dc40286`.
"Adapter-owned" = stays in `tuiscotti-runtime` (no termpane API
needed, by design).

**Architectural verdict (stands, narrowed):** `PtySession` is **not**
a drop-in for `tui::Session`. Merged `main` adds `observe()` and
deadline waits (drift D2), but `Observation` still carries no
title/bells/palette, `ModeState` still lacks 4/6/20, `Signal` still
lacks `Custom(i32)`, there is still no cancellation, and
`PtySession::spawn` still injects `COLORTERM=truecolor` with no unset
(`overrides_key` checks overrides only — `env_remove` does not
suppress it). The swap keeps tui-snap's own `Session`/worker/waits
skeleton and re-implements its *internals* on termpane **primitives**
(`pty` + `process` + `DamageGrid`). Every verdict below assumes that.

### Transport (launch/read/write/resize/identity/exit) — all COVERED

| # | Capability (today) | Verdict | termpane path |
|---|---|---|---|
| T1 | Launch: openpty + spawn + reader/writer + pid, one lifecycle lock (`builder.rs` `spawn()` L205 + `prepare_command` L275–296, `spawn.rs` `spawn_pty_child` L34–84) | COVERED (strictly better: drops parent slave + holds two locks — PTY lifecycle and transport-spawn — before returning) | `pty::spawn_pty(&SpawnParams, cols, rows) -> (Master, PtyChild)` (`pty.rs` L173); `Master::try_clone_reader/take_writer` (L228/L245); `SpawnParams::new/arg/args/env/env_clear/env_remove/current_dir/detached` + getters (`process.rs` L88–216) |
| T2 | Blocking read pump, EOF = `Ok(0)` (macOS) or EIO (Linux) (`capture.rs` L19–42) | COVERED, identical convention (documented `pty.rs` L18–19, L222) | `PtyReader: std::io::Read` (`pty.rs` L339–351) |
| T3 | Stdin write + close-input-by-drop (`worker_ctx.rs`) | COVERED | `PtyWriter: std::io::Write` (`pty.rs` L363–380) |
| T4 | Resize PTY-first, then grid (`encode.rs` `apply_resize` L144–170) | COVERED. **Pitfall:** `Master::resize(cols, rows)` but `DamageGrid::set_size(rows, cols)` — opposite order, document at call site | `Master::resize` (`pty.rs` L260) + `DamageGrid::set_size` (`grid.rs` L868; clamps to ≥1×1) + `Master::size` readback (`pty.rs` L277) |
| T5 | Child identity (`Session::pid`) | COVERED | `PtyChild::pid() -> Option<u32>` (`pty.rs` L402) |
| T6 | Exit poll/reap (`worker.rs` `poll_child` L284–295) | COVERED, same code/signal/success contract | `PtyChild::try_wait/wait` (`pty.rs` L422/L435) + `process::ExitStatus::{exit_code,signal,success}` (`process.rs` L306–320) |
| T7 | Bounded kill + reap (`worker.rs` `shutdown_child` L296–333) | COVERED. Verified-identical constants: `DRAIN_GRACE` 500 ms, `WORKER_TICK` 25 ms, `KILL_GRACE` 2 s, `REPLY_TIMEOUT` 10 s (`session_worker.rs` L71–77). **Correction:** termpane has no `JOIN_GRACE` — that identity claim in the old map is withdrawn; joins stay adapter-side | `PtyChild::kill` (`pty.rs` L452); adapter ports its loop verbatim; reference impl `session_worker.rs` `poll_exit_state` (L915) + `shutdown_child` (L982) |
| T8 | Signals incl. `Custom(i32)` (`session_input.rs` L296–308, worker-routed `CtlOp::Signal` via LIFE-2 sole reaper) | COVERED, as real syscalls (no more subprocess) | `process::signal(pid, signo)` (`process.rs` L467) + `SIGINT/SIGTERM/SIGKILL/SIGQUIT/SIGHUP` consts from `nix` (`process.rs` L51–59); `SignalError::{NotFound{pid},UnknownPid,InvalidSignal(i32),Failed{pid,message}}` (L410–431) maps to `ChildExited`/`Signal` exactly like today |
| T9 | Liveness probe (`exit.rs` L101–114, `kill -0` subprocess) | COVERED. **Semantic note:** EPERM reads *alive* in termpane (`pid_alive`, `process.rs` L497–509) but *absent* in today's `kill`-based probe (documented `exit.rs` L95–98); zombies count alive; pid 0 reads false on both sides. Callers only probe owned children, so no behavior change in practice — pin with a test | `process::pid_alive` (`process.rs` L497) |
| T10 | Drain discipline (handles-before-wait, EOF/EIO, trailing-output grace) | COVERED (pattern port; termpane's own worker, `session_worker.rs` `run_worker` L489 + `route_reply` L729–745, is the reference) | primitives above |

### Session machinery — ADAPTER-OWNED (keep as-is)

| # | Capability | Verdict |
|---|---|---|
| S1 | Cancellation (`CancelToken` through every wait) | No termpane API (`wait_exit`/`wait_revision`/`wait_frame` are deadline-only). Keep `Shared` + `wait_loop`; poll `PtyChild` in the same slices |
| S2 | Revisions, monotonic publish, `Condvar` waits, evidence-on-timeout | Same — tui-snap layer stays, fed by termpane-sourced observations. (Do not adopt termpane's revision counter: its bump points — feed batch, resize, color update, stdin close — do not match tui-snap's publish points, and mixing the two orderings breaks wait predicates.) |
| S3 | `finish`/`close`/`Drop` semantics, teardown-error recording, bounded joins | Same — port onto `PtyChild`; keep thread names and diagnostic strings (tests pin some) |

### Containment — PARTIAL, no change required

| # | Capability | Verdict |
|---|---|---|
| C1 | Child pgid/sid capture, own-pgid guard, per-pid reverify, per-pid SIGKILL (`unix.rs`) | COVERED: `process::process_ids` / `own_pgid` / `session_id_of` / `signal` / `own_pid` (`process.rs` L526–563). Optional follow-up: swap the `ps`/`kill` subprocesses for these syscalls |
| C2 | Full group-member enumeration (`ps -ax` snapshot, `snapshot()`) + start-time (`lstart`) reuse guard | GAP, but **no action needed**: `unix.rs` uses no banned dep, so the swap does not touch it. Keep the `ps` scan; optionally reverify via `session_id_of` |

### Observation state — mostly COVERED, three real GAPs

| # | Capability (today) | Verdict | termpane path / gap |
|---|---|---|---|
| O1 | Grid cells: wide pairing, colors, mods, underline style+color (`frame.rs` L74–228) | COVERED (superset) | `DamageGrid::dump() -> GridSnapshot` (`grid.rs` L679): `SnapCell{text,is_wide,is_wide_continuation,fg,bg,attributes,underline_style,underline_color,hyperlink_id,hyperlink_uri}` (`snapshot.rs` L64–84); `GridSnapshot` also carries `row_wraps` (ignore or adopt — decide at swap) |
| O2 | Cursor position | COVERED | `cursor_position()` (`grid.rs` L915) / `GridSnapshot.cursor` |
| O3 | Cursor style/visibility/blink (`frame.rs` L230–264) | COVERED with adapter logic | `cursor_style()` DECSCUSR u16 (`grid.rs` L935) + `hide_cursor()` (L974) + `text_cursor_enable()` (L1014) + mode 25 via `decrqm_status` |
| O4 | Modes 1,7,66,1000/1002/1003,1004,1005/1006,1049,2004,57399-kittypush (`frame.rs` L267–287) | COVERED | `application_cursor/keypad`, `autowrap`, `mouse_protocol_mode/encoding`, `focus_events`, `alternate_screen`, `bracketed_paste` (`grid.rs` L964–1006), `kitty_kb_flags() != 0` (L1099), `decrqm_status(mode)` (L1053) as cross-check |
| O5 | Modes **4 (IRM), 6 (DECOM), 20 (LNM)** | **GAP (re-confirmed).** Absorbed (`set_dec_mode` falls through to `_ => {}`, `grid.rs` L2078–2079; `decrqm_status` returns 0; conformance test at `grid/tests.rs:1381` pins IRM untracked; `ModeState` has no such fields). No current test asserts 4/6/20 (only 1004/2004 in `tests/tui/input.rs`), but `TermState.modes`/`TermSnapshot.modes` lose these numbers — a contract change to record (sibling G4 owns LIMITATIONS.md; handoff flagged) | — |
| O6 | Palette: OSC 4 overrides + OSC 4 query replies (`frame.rs`, `capture.rs` L75–81, L109–124) | **GAP, sharpest (re-confirmed).** Neither stored (`handle_osc`, `grid.rs` L1737–1835, has no `4` arm; unhandled OSC is dropped) nor surfaced in `ColorState` (reported fg/bg + current fg/bg only — `session_observe.rs` L41–49). **Repro:** `tests/tui_shell/state.rs::live_title_bells_modes_palette` (OSC 4 set → `assert_palette_entry`) and `tests/tui/session.rs:18` (`palette.is_known()`) fail under termpane. Options: (a) upstream OSC 4 support (new external ask), (b) contract change (`Known(empty)` + test updates — needs explicit approval, it is a behavior regression), (c) rejected: adapter-side pre-scan parser (divergent second parser — worse than the gap) | — |
| O7 | Default fg/bg: OSC 10/11 **set** forms (`replay_state.rs`) | **GAP (re-confirmed).** Set forms explicitly dropped (`grid.rs` L1775–1780: only `?` queries answered, from capsule-set — not program-set — colors). Live path already reports `Unsupported` (no live test pins defaults); replay `TermSnapshot.defaults` coverage must be re-verified at swap time | — |
| O8 | Title + bells (`capture.rs` L55–57, `WorkerEventState`) | COVERED with adapter rules: accumulate `PassthroughEvent::TitleChanged/Bell` (`passthrough.rs` L18–20) into worker state; map `TitleChanged("")` → `None` to preserve alacritty `ResetTitle` semantics; `IconNameChanged` (OSC 1 — termpane models it, alacritty folded it into `Title`): map into title as today, or ignore — decide at swap, pin with a test. (Neither `GridSnapshot` nor `Observation` carries title/bells — the worker-state design is unchanged.) | `drain_passthrough` (`grid.rs` L642) |
| O9 | Clipboard OSC 52 (`replay_screen.rs`, `SandboxClipboard`) | COVERED with adapter decode: `ClipboardWrite("<sel>;<b64>")` (`passthrough.rs` L23–26); base64-decode via the already-present `base64` dep; selection map `c→Clipboard, p|s→Selection` (matches alacritty 0.26 `term/mod.rs:1712–1713`, re-verified in registry source) | `drain_passthrough` |
| O10 | Hyperlinks OSC 8 (`replay_state.rs`) | COVERED (richer: id + uri) | `Cell.hyperlink{…}` (`cell.rs` L117–118), `SnapCell.hyperlink_id/uri`, `hyperlink_target_at_content_row` (`grid.rs` L1170); keep dedupe-by-uri + 1024 cap |
| O11 | Scrollback text (`replay_state.rs`) | COVERED | `scrollback_rows_at_offset` (L751), `dump_scrollback_view` (L775), `scrollback_len` (L940); replay limit stays 1000 (`DamageGrid::new(rows, cols, 1000)`). Live alacritty default was 10000 but unobservable — keep either, note the choice |
| O12 | Query replies DA1/DA2, DSR 5n/6n/?6n, DECRQM, kitty `?u`, OSC 10/11 query (today `Event::PtyWrite`/`ColorRequest` → stdin) | COVERED: identical routing shape (worker routes `Reply` bytes to stdin, stashes the rest — reference `session_worker.rs` L702/L729–745) | `PassthroughEvent::Reply` (`passthrough.rs` L57–64); emitters `grid/perform.rs` L380–441, `grid.rs` L1782–1791 |
| O13 | Text-area-size replies CSI 14/16/18t (`capture.rs` L83–95) | **GAP, minor (re-confirmed).** termpane never answers them. No test/fixture queries them; impact is limited to programs that block on that answer. Record in LIMITATIONS (handoff) | — |
| O14 | Clipboard **read** answers OSC 52 `?` (today: honestly-empty reply, `capture.rs` L67–73) | COVERED with adapter rule: `ClipboardWrite` payload `"<sel>;?"` → reply the empty-store sequence, preserving today's behavior | `drain_passthrough` + writer |
| O15 | Synchronized output DEC 2026 (today: rejected — `tui/profile.rs` L76–81, `wait_frame` always `Unsupported` (`session.rs` L204–213), pinned by `tests/tui/input.rs:55` + `:281`) | **IMPROVEMENT available, now cheaper:** termpane tracks it (`in_synchronized_update`, `grid.rs` L1021; `decrqm_status(2026)`) **and** ships `PtySession::wait_frame` + `sync_frames_completed` (drift D6). Recommend **two-phase**: swap keeps `wait_frame` fail-closed + rejection (approval stability), follow-up adopts termpane's frame wait + flips those tests | `in_synchronized_update` |
| O16 | Per-cell blink SGR 5/6 (today: dropped — `frame.rs` L162, `profile.rs` L84–86, pinned by `tests/tui/input.rs:284` + `:291`) | **IMPROVEMENT, approval-affecting:** termpane tracks `slow_blink`/`rapid_blink` per cell (`cell.rs` L55–57, `SnapCellAttrs`), so `Mods.blink` becomes populated and the `cell_blink` profile rejection **lifts** (the `input.rs:284` test flips to accept). No fixture app emits blink, so no `.snap` churn expected — but any blink-byte test vectors change content | — |
| O17 | Overline SGR 53 | Dropped (no `Mods` field; same class as before, termpane just tracks more than we project). Note in LIMITATIONS (handoff) | — |

### Input encoding — COVERED, adapter-owned on termpane mode getters

| # | Capability (today) | Verdict | termpane path |
|---|---|---|---|
| I1 | Key encoding: kitty `CSI u` vs legacy, app-cursor arrows (`encode_key.rs`) | COVERED. Kitty-active ⟺ `kitty_kb_flags() != 0`; app-cursor ⟺ `application_cursor()`. Parity check at swap: kitty push/pop fixtures must encode identically (termpane answers `?u` queries with `?0u` — `grid/perform.rs` L380 — programs that *push* unilaterally, the common case, are unaffected) | `kitty_kb_flags` (`grid.rs` L1099), `application_cursor` (L984) |
| I2 | Mouse encoding: SGR/UTF-8/X10 + mode gates 1000/1002/1003 (`encode.rs` L99–206) | COVERED. termpane adds a `Urxvt` (1015) encoding variant (`grid.rs` L93–102) with no tui-snap counterpart: map to legacy X10 (documented, pinned by test) | `mouse_protocol_mode/encoding` (`grid.rs` L964 + encoding getter) |
| I3 | Paste (bracketed iff 2004, delimiter rejection) | COVERED | `bracketed_paste()` (L979); keep `PasteRejected` + `is_focused` handling |
| I4 | Focus (refused unless 1004) + worker-side focus state | COVERED | `focus_events()` (L992); focus flag stays adapter-side (termpane needs no equivalent — it never reads host focus) |
| I5 | Raw bytes/text passthrough | COVERED | direct `PtyWriter` write |

### Replay/record — COVERED

| # | Capability | Verdict | termpane path |
|---|---|---|---|
| R1 | `Recording`/`replay_*`/chunk invariance/`MAX_REPLAY_BYTES` (`replay_api.rs`) | COVERED. `DamageGrid::process(&mut self, bytes)` keeps a persistent `vte::Parser` + `pending_utf8` across calls (`grid.rs` L594) — split-sequence safety is structural, same property the `replay_chunk_invariance_all_split_points` test pins | `DamageGrid::process` + `drain_passthrough` + `dump` |
| R2 | Replay screen/state builders | COVERED: same projection code as O1–O11, minus the worker | — |
| R3 | Shell marker protocol (OSC 133 C/D + text attestations, `shell.rs`) | COVERED trivially: parsing is on viewport text rows, and unhandled OSC (incl. 133) is safely dropped (`grid.rs` L1834 `_ => {}`) | — |

### Environment / platform scope — one GAP, one decision

| # | Capability | Verdict |
|---|---|---|
| E1 | argv-verbatim, child-only env + `TERM` default, child cwd, no parent mutation (`builder.rs` L237–251) | COVERED: `SpawnParams` (verbatim argv, overrides-only env, `current_dir`) + adapter-set `TERM`. **Do not adopt** `PtySession::spawn` (`SessionOptions.colorterm` `"truecolor"` default, injected unless overridden — `session.rs` L268–269 — and `overrides_key` ignores `env_remove`, so there is still no unset): tui-snap exports no `COLORTERM` today, so the raw-`spawn_pty` architecture is required to preserve this. Note the backend caveat in the `spawn_pty` docs: `portable-pty` always re-adds `SHELL`, so `env_clear`/`env_remove("SHELL")` cannot drop it — pin with a test if any fixture asserts a cleared env |
| E2 | Non-Unix builds (Windows stubs: `signal`, `process_exists`, guardian) | **GAP / scope decision (re-confirmed).** `termpane` `process`/`pty`/`session` are Unix-only with `compile_error!` on other targets (`lib.rs` L25–28). `portable-pty`/`alacritty_terminal` build on Windows today. The swap must Unix-gate the `pty` feature (target-gated optional dep + `cfg(unix)`; `dep:` refs to target-specific optional deps resolve to nothing off-target — verify at swap time) and keep/extend the non-Unix stubs. If Windows PTY support is a requirement, that is a second upstream ask |
| E3 | `MIN_COLS = 2` (alacritty floor, `limits.rs` L11) vs termpane `MIN_COLS = 1` (`session.rs` L55; `check_size` rejects only 0, `pty.rs` L86–91) | Relaxation opportunity: adopt 1 (matches the replay path's 1..=1000 and static 1×1 screens). Small behavior expansion; update `Tui::size`/`resize` validation + tests |

## 3. Swap procedure (runs only after §0 clears)

Preconditions: a crates.io `termpane` release `=X.Y.Z` whose API
covers every COVERED row above (re-run §2 against the release — the
map is pinned to `dc40286`, not to "whatever released").
`cargo publish` rights are upstream's; we only consume.

1. **Manifests.**
   - Root `Cargo.toml` `[workspace.dependencies]`: delete the three
     entries + temp comment (L32–35); add `termpane = "=X.Y.Z"`
     (exact pin, per repo policy on released versions).
   - `crates/tuiscotti-runtime/Cargo.toml`: `pty =
     ["dep:termpane"]`; replace the three optional deps with a
     Unix-gated `termpane = { workspace = true, optional = true,
     features = ["pty"] }` under `[target.'cfg(unix)'.dependencies]`
     (E2). Verify off-Unix `cargo check` still passes via stubs.
   - **Forbidden in this step:** `git =`, `path =`, `[patch]`,
     version `"*"`, or a version range — see §7.
2. **Enforcement flip.** `crates/xtask/src/deps.rs`: delete
   `TEMP_HOLDER` (L26) and the `TEMP-ALLOW` branch (L155–156) so any
   direct occurrence of the three names fails in every manifest.
   `BANNED` stays.
3. **Live path rewire** (`src/tui/`, public API unchanged):
   `builder.rs` (`Tui::spawn` → `spawn_pty` + `SpawnParams`; keep
   `TERM`-defaulting, cargo-bin resolution, `await_initial`);
   `worker.rs`/`worker_ctx.rs` (`Term<QueueListener>` →
   `DamageGrid`, `Processor::advance` → `grid.process`,
   `MasterPty`/`Child` trait objects → `Master`/`PtyChild`,
   `shutdown_child`/`poll_child` ported); `capture.rs`
   (`drain_term_events` → `drain_passthrough` + Reply routing + O8
   accumulation); `frame.rs` (projection onto `GridSnapshot` per
   O1–O4, O8, O15–O17); `encode.rs`/`encode_key.rs` (mode bits from
   I1–I4 getters); `exit.rs` (`From<termpane::process::ExitStatus>`,
   `process_exists` → `pid_alive` per T9 note); `input_types.rs`
   (drop `libc`, delete `Signal::number` or map to `process::SIG*`);
   `session_input.rs` (`signal()` → `process::signal`, `Custom(n)`
   passes through); `limits.rs` (`MIN_COLS` 2→1 per E3; drop
   `PTY_LIFECYCLE` — the guard moves inside termpane); `mod.rs` docs.
   Keep: `session.rs`, `session_teardown.rs`, `shared.rs`, `error.rs`,
   thread architecture, timing constants, diagnostic strings.
4. **Replay rewire** (`src/tui_shell/replay_*.rs`): fresh
   `DamageGrid::new(rows, cols, 1000)` + `process` +
   `drain_passthrough` + `dump`; same projections as step 3 (R1–R2,
   O9–O11). Keep `Recording`/`Replayed`/`MAX_REPLAY_BYTES`/caps
   byte-identical.
5. **Test helper**: `tests/tui_shell/replay.rs::capture_raw` drops
   `use portable_pty` → same shape on `termpane::pty` (`spawn_pty` +
   reader thread + bounded drain).
6. **Contract-change tests** (behavior deltas needing explicit
   sign-off at swap time, not silent fixes):
   `state.rs::live_title_bells_modes_palette` + `session.rs:18` (O6
   palette gap — resolve per the chosen O6 option first);
   `input.rs:284` (O16 blink rejection lifts — flip to accept, add a
   blink-bytes vector); `input.rs:55` + `:281` stay red *unchanged*
   (O15 two-phase: still `Unsupported`); add pins for O8 (`""`→None,
   OSC 1 rule), I2 (Urxvt→X10), T9 (EPERM/zombie/pid-0 semantics —
   EPERM case needs a foreign-uid helper or stays a doc pin),
   O9 (selection map + `?`-read empty reply), E1 (`SHELL` re-add
   caveat if any fixture asserts cleared env), E3 (`MIN_COLS` 1).
7. **Mechanical sweep**: update every §1.2 comment mention; delete
   now-dead `use` lines; `cargo +nightly fmt` + repo lints.
8. **Lockfile**: `cargo update -p termpane` (or full `cargo update`
   if policy allows); confirm `alacritty_terminal` leaves the graph
   and `portable-pty`/`libc`/`nix` remain only transitively
   (`cargo tree -i` per §1.4).

## 4. Verification (swap-day gates)

Run in order; each gate must be green before the next starts:

1. `cargo metadata` / `cargo tree` gates from §1.4: no direct edge
   to any banned crate from any workspace member; `--no-default-
   features` graph unchanged (pure view).
2. `cargo xtask deps` (post-flip): must pass with no `TEMP-ALLOW`
   output.
3. Full workspace test suite with default features, then with
   `--no-default-features` (pure view), on Linux and macOS.
   Expected reds are exactly the §3-step-6 contract list — any other
   failure is a swap bug, not a delta.
4. Replay invariance suite (`replay_chunk_invariance_all_split_points`
   et al.) — structural R1 proof on the new parser.
5. Snapshot review: no `.snap` churn except blink-byte vectors
   (O16) and any approved O6 contract change. Each churned snap
   needs a one-line justification naming the map row.
6. Off-Unix `cargo check` (E2 stubs) — at minimum
   `cargo check --target x86_64-pc-windows-msvc` if the toolchain
   is available, else `cfg(not(unix))` code review + CI.
7. New pins from §3-step-6 all pass; O5/O7/O13 LIMITATIONS entries
   land (see §5) before merge.

## 5. Handoff (approvals + sibling work)

- **O6 decision** (upstream ask vs `Known(empty)` contract change):
   needs explicit human approval at swap time. This is the only
   swap item that can regress released behavior; do not default it.
- **O16 flip** (blink rejection lifts): approval-affecting but a
   strict improvement; bundle with the swap PR, call it out in the
   description.
- **O5 / O7 / O13 / O17**: record in LIMITATIONS.md — owned by
   sibling G4. File the handoff with the exact mode numbers and
   repro programs (`printf '\e[4h'` DECRQM reads 0; OSC 4 set+query;
   CSI `18t` unanswered; SGR 53 dropped).
- **O15 follow-up**: adopt `PtySession::wait_frame`-equivalent on
   the raw-`spawn_pty` architecture (or justify adopting
   `PtySession` wholesale then) + flip `input.rs:55`/`:281`.
   Separate PR after the swap lands.
- **C1 follow-up (optional)**: replace `ps`/`kill` subprocesses
   with `process::process_ids/signal` syscalls. Not part of the swap.

## 6. Release watch (until §0 clears)

Poll weekly (or on upstream release notifications):

```sh
curl -s -A 'tui-snap-plan-check' -o /dev/null -w '%{http_code}\n' \
  https://crates.io/api/v1/crates/termpane       # want 200
curl -s -o /dev/null -w '%{http_code}\n' \
  https://index.crates.io/te/rm/termpane         # want 200
```

When 200: fetch the release source, re-run the §2 map row by row
(API drift check — see Appendix A for the previous round's drift),
then execute §3. Do not start §3 on a yanked or pre-release version.

## 7. Forbidden bridges (why the swap waits for the registry)

- No `git =` dependency on `tailrocks/termpane`: unpinned,
   unreviewable, breaks the offline/locked build story.
- No `path =` / vendor copy: forks the API surface; drift
   detection (§6) becomes meaningless.
- No `[patch]`: silently redirects every consumer; hides the real
   dependency graph from `cargo tree` audits.
- No version range or `"*"`: the §2 map is pinned to an exact
   source revision; a range can resolve to an unmapped API.

## Appendix A. Drift log: `dad8389` pins → `dc40286` merged

(`dad8389` is not in the qual clone — squash merge — so "before" is
the old document's pinned claims, "after" is merged source.)

| # | Area | Drift |
|---|---|---|
| D1 | Session layer split | `session.rs` worker/kill/Reply code moved to `session_worker.rs` (`run_worker` L489, `route_reply` L729–745, `poll_exit_state` L915, `shutdown_child` L982) and `session_observe.rs`. Old pins `session.rs` L855–873 / L908–933 / L937–972 are stale. |
| D2 | `observe()` + waits are new | `PtySession::observe() -> Observation` (`session.rs` L439; `Observation{revision,grid,cursor,colors,modes,capabilities,completeness,diagnostics}`, `session_observe.rs` L252) plus `wait_revision` (L453) / `wait_exit` (L704) / `poll_exit` (L685), all deadline-based. Old "no revisions or waits" verdict is narrowed: still no cancellation, still no title/bells/palette in the observation, still no 4/6/20 in `ModeState`. Swap architecture unchanged. |
| D3 | `SpawnParams` grows env control | New `env_clear` / `env_remove` / `is_env_cleared` / `env_removed` (`process.rs` L151–207). E1 "no unset" still holds for the `PtySession` path (`overrides_key` ignores removals, `session.rs` L211–216); raw `spawn_pty` honors clear→removals→overrides, with the documented `SHELL` re-add caveat. |
| D4 | `PassthroughEvent` grows | Beyond `Bell/TitleChanged/IconNameChanged/ClipboardWrite/Reply`: `CwdChanged`, `Notification`, `ApplicationCursorKeys`, `FocusEvents`, `BracketedPaste`, `Hyperlink{id,uri}`, `DroppedCsi`, `ScrollbackClear`. Adapter must ignore (or explicitly handle) the new variants — match exhaustively. |
| D5 | T7 correction | termpane has no `JOIN_GRACE`; old "identical JOIN_GRACE 5 s" claim withdrawn. Verified-identical: `DRAIN_GRACE`, `WORKER_TICK`, `KILL_GRACE`, `REPLY_TIMEOUT` (`session_worker.rs` L71–77). |
| D6 | `wait_frame` exists | `PtySession::wait_frame` (`session.rs` L482) + `sync_frames_completed` diagnostics. O15 follow-up is now a thin adoption, not new tracking. Two-phase recommendation unchanged. |
| D7 | Line shifts | `spawn_pty` L154→L173; `process.rs` +~14–60 (`signal` L409→L467, `pid_alive` L439→L497, id probes L453–516→L526–563); `passthrough.rs` variants L20–79→L18–64. All `grid.rs` getter pins (L594–L1170) and `handle_osc` (L1737) / `set_dec_mode` fallthrough (L2078) / `perform.rs` replies (L380+) verified stable. |
| D8 | `GridSnapshot`/`SnapCell` | `GridSnapshot` gains `row_wraps`; hyperlink split into `hyperlink_id` + `hyperlink_uri` (`snapshot.rs` L82–84) — consistent with the old `hyperlink_*` shorthand. |
| D9 | tui-snap-side drift (`75ff479`→`f89d522`) | `worker.rs` poll/shutdown L177–226→L259–300; `session_input.rs` signal L268–292→L303–328; `exit.rs` gains pid-0 guard + EPERM doc (L95–114); `input.rs` pins `:274`→`:281`, `:284`+`:291`; `proto/*.rs` comment mentions gone. No new holders; file count unchanged (13). |
| D10 | Release metadata | termpane `version = "0.1.0"`, `rust-version = "1.97"` (compatible with our 1.98 floor), `nix 0.31`, `portable-pty 0.9`. First release to watch is presumably `0.1.0`. |

Gaps O5/O6/O7/O13 and E2 re-confirmed present in merged `main`;
no gap was closed by the merge. Verdicts on all other rows re-confirmed.
