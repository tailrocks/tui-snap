# G1 termpane backend swap — assessment + durable plan

Branch: `redesign/rust-first-testing-platform` (assessed at `75ff479`).
Upstream reference: `tailrocks/termpane` PR #25, branch
`feat/process-pty-transport`, commit `dad8389`
(`dad838982aafb5784f6354b7ca0c030eaa3c1dcc`), checked out read-only at
`/tmp/termpane-pr25`. PR base: `main` at `8ff87fe`.

Target end state: **no direct dependency** on `portable-pty`,
`alacritty_terminal`, or `libc` anywhere in the workspace; the PTY
backend comes only from a **released** `termpane` from crates.io.
(`libc`/`portable-pty` remain in the graph *transitively* regardless —
see §2. Transitive is permitted; direct is banned.)

## 0. External blocker — the swap cannot land yet

Three stacked facts, all verified 2026-09-29:

1. **PR #25 is open and unmerged.** GitHub API: `"state":"open"`,
   `"merged_at":null`, head `dad8389...`. The `process` / `pty` /
   `session` modules exist only on `feat/process-pty-transport`, not on
   `main` (base `8ff87fe`).
2. **PR #25 is green.** All 8 check-runs at `dad8389` are
   `completed`/`success` (`Control / Required`, `ci-required`,
   `rust-policy`, `rust-termpane`, `rust-termpane-fuzz`, `Control /
   Planning`, `Policy`, `DCO`).
3. **`termpane` has zero crates.io releases.** Both
   `https://crates.io/api/v1/crates/termpane` and the sparse index
   `https://index.crates.io/te/rm/termpane` return **404**. There is no
   published version to pin — not even one predating PR #25.

Plus a permission fact: this project cannot publish `termpane`
itself (upstream `tailrocks` owner must merge + release). Until a
registry release containing the PR #25 API surface exists, the swap is
**externally blocked**. A git or path dependency is not an acceptable
bridge (§7). This document is evidence + plan, not the swap.

## 1. Inventory: every occurrence of the three crates

### 1.1 Direct manifest dependencies

Exactly one holder, both seams documented as temporary:

| Manifest | Occurrence |
|---|---|
| `Cargo.toml` `[workspace.dependencies]` | `portable-pty = "=0.9.0"`, `alacritty_terminal = "=0.26.0"`, `libc = "=0.2.189"` (+ comment "temporarily retained under tuiscotti-runtime only (G1 swap later)") |
| `crates/tuiscotti-runtime/Cargo.toml` | `pty = ["dep:portable-pty", "dep:alacritty_terminal", "dep:libc"]` (line 12); three `optional = true` deps (lines 22–24) |

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

13 files. All inside `tuiscotti-runtime` (12 under `src/`, 1 test):

**`src/tui/` — live session path**

| File | `portable-pty` uses | `alacritty_terminal` uses | `libc` uses |
|---|---|---|---|
| `tui/builder.rs` | `CommandBuilder`, `PtySize`, `native_pty_system` (L9); `Box<dyn MasterPty>` / `Box<dyn Child>` (L254–255); openpty/spawn/reader/writer/poll/pid (L263–300) | `TermConfig` (L8, L185–188) | — |
| `tui/worker.rs` | `Child as PtyChild`, `MasterPty` (L9); `portable_pty::ExitStatus` (L179); `try_wait`/`kill` in `poll_child`/`shutdown_child` (L177–226) | `Event`, `EventListener`, `GridDims`, `TermConfig`, `Term` (L6–8); `Term::new` (L142) | — |
| `tui/worker_ctx.rs` | `Child as PtyChild`, `MasterPty` (L13, L42–43) | `Event`, `Term`, `Processor` (L10–12); `processor.advance` feed (L105) | — |
| `tui/capture.rs` | — | `Event`, `EventListener`, `WindowSize`, `Term`, `VteRgb` (L5–7); reply/color/size drain (L45–103) | — |
| `tui/encode.rs` | `MasterPty`, `PtySize` (L6, L42–62) | `EventListener`, `GridDims`, `Term`, `TermMode` (L3–5); mode-gated encodings (L24–40); `term.resize` (L57–60) | — |
| `tui/encode_key.rs` | — | `TermMode` (L3): `KITTY_KEYBOARD_PROTOCOL` (L27), `APP_CURSOR` (L75) | — |
| `tui/exit.rs` | `From<portable_pty::ExitStatus>` (L43–50) | — | — (comment only, L105) |
| `tui/frame.rs` | — | `EventListener`, `index::{Column,Line}`, `Term`, `TermMode`, `CellFlags`, `VteColor`, `CursorShape`, `NamedColor` (L7–12) + fully-qualified `term::cell::Cell` (L143); modes→numbers map (L267–290) | — |
| `tui/input_types.rs` | — | — | `libc::SIGINT/SIGTERM/SIGKILL/SIGQUIT/SIGHUP` (L245–249, fully qualified; no `use libc`). Only direct `libc` code use in the repo |

**`src/tui_shell/` — replay path** (live `Shell`/`Guardian` touch no
banned crate; containment is `ps`/`kill` subprocesses in `unix.rs`)

| File | Uses |
|---|---|
| `tui_shell/replay_api.rs` | `Event`, `EventListener`, `GridDims`, `TermConfig`, `Term`, `Processor` (L3–6); fresh-emulator replay (L212–226) |
| `tui_shell/replay_screen.rs` | `Event`, `EventListener`, `index`, `CellFlags`, `ClipboardType`, `Term`, `TermMode`, `VteColor`, `CursorShape`, `NamedColor` (L3–7) + FQN cell (L92) |
| `tui_shell/replay_state.rs` | `EventListener`, `GridDims`, `index`, `CellFlags`, `Term`, `TermMode`, `NamedColor`, `VteRgb` (L3–8) + FQN grid (L104, L126) |

**Tests + tooling**

| File | Uses |
|---|---|
| `tests/tui_shell/replay.rs` | `use portable_pty::{CommandBuilder, PtySize, native_pty_system}` (L45) in the `capture_raw` helper — compiles only because integration tests link the package's (optional, default-on) deps; gated by `#![cfg(feature = "pty")]` |
| `crates/xtask/src/deps.rs` | Name strings in `BANNED` (L23) + `TEMP_HOLDER` exception (L26, L155–158). Updated by the swap, not deleted |

Comment/doc-only mentions (updated mechanically with the swap):
`tui/mod.rs` (L3, L13), `tui/limits.rs` (L7, L10),
`tui/session_input.rs` (L275), `proto/sessions.rs`,
`proto/session_ops.rs`, `tui_shell/unix.rs`, `frame.rs` (L162, L177),
`replay_screen.rs` (L125), `tuiscotti-fixtures/.../consumer/main.rs`
(L4), `tuiscotti-fixtures/tests/underline.rs` (L9).

### 1.3 Transitive graph (locked)

`Cargo.lock`: `portable-pty 0.9.0`, `alacritty_terminal 0.26.0`,
`libc 0.2.189`, all from the crates.io registry. Reverse edges:

- `portable-pty`: sole user `tuiscotti-runtime`. After the swap it
  re-enters **transitively** via `termpane` (`pty` feature pulls
  `portable-pty 0.9` — see `/tmp/termpane-pr25/Cargo.toml`).
- `alacritty_terminal`: sole user `tuiscotti-runtime`. Leaves the graph
  (nothing else wants it).
- `libc`: **21 users** — direct `tuiscotti-runtime` plus
  `alacritty_terminal`, `cpufeatures`, `errno`, `filedescriptor`,
  `getrandom`, `mio`, `nix`, `num_threads`, `parking_lot_core`,
  `portable-pty`, `rustix`, `rustix-openpty`, `serial2`,
  `shared_library`, `signal-hook`, `signal-hook-mio`,
  `signal-hook-registry`, `termios`, `termwiz`, `time`. It stays locked
  via `crossterm`/`tempfile`/`sha2`/… regardless of the swap, and
  `termpane process` adds `nix 0.31 → libc` too. The goal is **no
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
  profile) → `spawn()`; `spawn_pty_child` (openpty + spawn + handles +
  pid under the global `PTY_LIFECYCLE` guard); reader+worker threads.
- ** Worker loop**: `tui/worker.rs` (`Op` enum, `run_worker`,
  `poll_child`, `shutdown_child`) + `tui/worker_ctx.rs` (`WorkerCtx`:
  owns `Term`, writer, child; feed/eof/observe/input/resize/
  close-input/shutdown/disconnect; exit poll with `DRAIN_GRACE`).
- **Reader/event drain**: `tui/capture.rs` (`run_reader`,
  `drain_term_events` — title/bell/reply/clipboard/color/size answers,
  `publish_exit`).
- **Observation build**: `tui/frame.rs` (`build_observation`: grid +
  cursor + palette + modes at one revision).
- **Input encoding**: `tui/encode.rs` + `tui/encode_key.rs` (mode-gated
  bytes from live `TermMode`); `tui/session_input.rs` (`Session` input
  methods; Unix `signal()` via `kill(1)`, `Signal::number()` is the
  only `libc` use).
- **Waits/teardown**: `tui/session.rs` (observe/waits incl.
  always-`Unsupported` `wait_frame`), `tui/session_teardown.rs`
  (`finish`/`close`/`Drop`, bounded joins), `tui/shared.rs`
  (revisioned publish + `Condvar`), `tui/exit.rs` (`ExitStatus`,
  `ExitWait`, `process_exists` via `kill -0`), `tui/error.rs`
  (`TuiError`, `WaitError`, `CancelToken`).
- **Shell/guardian**: `tui_shell/shell.rs` (OSC 133 marker protocol over
  `Session`), `tui_shell/guardian.rs` + `tui_shell/unix.rs`
  (process-group sweep via `ps`/`kill` — no banned deps).
- **Replay**: `tui_shell/replay_api.rs` (`Recording`, `replay_*`,
  `MAX_REPLAY_BYTES`, chunk invariance) + `replay_screen.rs` +
  `replay_state.rs` (fresh `Term`, `REPLAY_HISTORY = 1000`).
- **State assertions**: `tui_shell/state.rs` (`TermSnapshot`,
  `SandboxClipboard`, `assert_*`).

Public API stability boundary (must not change shape):
`tuiscotti::tui::{Tui, Session, …}` facade re-exports,
`Observation`/`TermState`, `TermSnapshot`, `Recording`/`Replayed`.

## 2. Coverage map: adapter capability → termpane PR #25 API

All paths verified against `/tmp/termpane-pr25/src` at `dad8389`.
"Adapter-owned" = stays in `tuiscotti-runtime` (no termpane API
needed, by design).

**Architectural verdict first:** `termpane::session::PtySession` is
**not** a drop-in for `tui::Session`. Its `snapshot()` returns only
`GridSnapshot` (cells + cursor pos + alt-screen flag — no modes, cursor
style/visibility, title, bells, palette), it has no revisions,
cancellation, or waits, its `Signal` lacks `Custom(i32)`, and it
injects `COLORTERM=truecolor` (behavior change; `SpawnParams` has no
unset). The swap keeps tui-snap's own `Session`/worker/waits skeleton
and re-implements its *internals* on termpane **primitives**
(`pty` + `process` + `DamageGrid`). Every verdict below assumes that.

### Transport (launch/read/write/resize/identity/exit) — all COVERED

| # | Capability (today) | Verdict | termpane path |
|---|---|---|---|
| T1 | Launch: openpty + spawn + reader/writer + pid, one lifecycle lock (`builder.rs` L263–300) | COVERED (strictly better: drops parent slave under the lock) | `pty::spawn_pty(&SpawnParams, cols, rows) -> (Master, PtyChild)` (`pty.rs` L154); `Master::try_clone_reader/take_writer` (L203/L219); `SpawnParams::new/arg/args/env/current_dir/detached` + getters (`process.rs` L74–161) |
| T2 | Blocking read pump, EOF = `Ok(0)` (macOS) or EIO (Linux) (`capture.rs` L17–42) | COVERED, identical convention (documented `pty.rs` L18–20) | `PtyReader: std::io::Read` (`pty.rs` L297–311) |
| T3 | Stdin write + close-input-by-drop (`worker_ctx.rs` L213–225) | COVERED | `PtyWriter: std::io::Write` (`pty.rs` L319–337) |
| T4 | Resize PTY-first, then grid (`encode.rs` L42–62) | COVERED. **Pitfall:** `Master::resize(cols, rows)` but `DamageGrid::set_size(rows, cols)` — opposite order, document at call site | `Master::resize` (`pty.rs` L234) + `DamageGrid::set_size` (`grid.rs` L868; clamps to ≥1×1) + `Master::size` readback (`pty.rs` L251) |
| T5 | Child identity (`Session::pid`) | COVERED | `PtyChild::pid() -> Option<u32>` (`pty.rs` L362) |
| T6 | Exit poll/reap (`worker.rs` L177–184) | COVERED, same code/signal/success/Display contract | `PtyChild::try_wait/wait` (`pty.rs` L371/L380) + `process::ExitStatus::{exit_code,signal,success}` (`process.rs` L240–266) |
| T7 | Bounded kill + reap (`worker.rs` L189–226) | COVERED. Timing constants are **identical**: `DRAIN_GRACE` 500 ms, `WORKER_TICK` 25 ms, `KILL_GRACE` 2 s, `JOIN_GRACE` 5 s, reply timeout 10 s | `PtyChild::kill` (`pty.rs` L389); adapter ports its loop verbatim; reference impl `session.rs` L937–972 |
| T8 | Signals incl. `Custom(i32)` (`session_input.rs` L268–292, via `kill(1)` + `libc` numbers) | COVERED, as real syscalls (no more subprocess) | `process::signal(pid, signo)` (`process.rs` L409) + `SIGINT/SIGTERM/SIGKILL/SIGQUIT/SIGHUP` consts from `nix` (`process.rs` L43–51); `SignalError::{NotFound,UnknownPid,InvalidSignal,Failed}` maps to `ChildExited`/`Signal` exactly like today |
| T9 | Liveness probe (`exit.rs` L101–111, `kill -0` subprocess) | COVERED. **Semantic note:** EPERM reads *alive* in termpane (`pid_alive`, `process.rs` L439) but *absent* in today's `kill`-based probe; zombies count alive. Callers only probe owned children, so no behavior change in practice — pin with a test | `process::pid_alive` (`process.rs` L439) |
| T10 | Drain discipline (handles-before-wait, EOF/EIO, trailing-output grace, `poll_exit_progress`) | COVERED (pattern port; termpane's own worker, `session.rs` L908–933, is the reference) | primitives above |

### Session machinery — ADAPTER-OWNED (keep as-is)

| # | Capability | Verdict |
|---|---|---|
| S1 | Cancellation (`CancelToken` through every wait) | No termpane API (`wait_exit(deadline)` has no cancel). Keep `Shared` + `wait_loop`; poll `PtyChild`/`poll_exit` in the same slices |
| S2 | Revisions, monotonic publish, `Condvar` waits, evidence-on-timeout | Same — tui-snap layer stays, fed by termpane-sourced observations |
| S3 | `finish`/`close`/`Drop` semantics, teardown-error recording, bounded joins | Same — port onto `PtyChild`; keep thread names and diagnostic strings (tests pin some) |

### Containment — PARTIAL, no change required

| # | Capability | Verdict |
|---|---|---|
| C1 | Child pgid/sid capture, own-pgid guard, per-pid reverify, per-pid SIGKILL (`unix.rs`) | COVERED: `process::process_ids` / `own_pgid` / `session_id_of` / `signal` / `own_pid` (`process.rs` L453–516). Optional follow-up: swap the `ps`/`kill` subprocesses for these syscalls |
| C2 | Full group-member enumeration (`ps -ax` snapshot, `snapshot()`) + start-time (`lstart`) reuse guard | GAP, but **no action needed**: `unix.rs` uses no banned dep, so the swap does not touch it. Keep the `ps` scan; optionally reverify via `session_id_of` |

### Observation state — mostly COVERED, three real GAPs

| # | Capability (today) | Verdict | termpane path / gap |
|---|---|---|---|
| O1 | Grid cells: wide pairing, colors, mods, underline style+color (`frame.rs` L74–228) | COVERED (superset) | `DamageGrid::dump() -> GridSnapshot` (`grid.rs` L679): `SnapCell{text,is_wide,is_wide_continuation,fg,bg,attributes,underline_style,underline_color,hyperlink_*}` (`snapshot.rs` L64–86) |
| O2 | Cursor position | COVERED | `cursor_position()` (`grid.rs` L915) / `GridSnapshot.cursor` |
| O3 | Cursor style/visibility/blink (`frame.rs` L230–264) | COVERED with adapter logic | `cursor_style()` DECSCUSR u16 (`grid.rs` L935) + `hide_cursor()` (L974) + `text_cursor_enable()` (L1014) + mode 25 via `decrqm_status` |
| O4 | Modes 1,7,66,1000/1002/1003,1004,1005/1006,1049,2004,57399-kittypush (`frame.rs` L267–290) | COVERED | `application_cursor/keypad`, `autowrap`, `mouse_protocol_mode/encoding`, `focus_events`, `alternate_screen`, `bracketed_paste` (`grid.rs` L964–1006), `kitty_kb_flags() != 0` (L1099), `decrqm_status(mode)` (L1053) as cross-check |
| O5 | Modes **4 (IRM), 6 (DECOM), 20 (LNM)** | **GAP.** termpane absorbs them (`set_dec_mode` falls through to `_ => {}`, `grid.rs` L2076; `decrqm_status` returns 0; conformance test at `grid/tests.rs:1381` pins IRM untracked). No current test asserts 4/6/20 (only 1004/2004 in `tests/tui/input.rs`), but `TermState.modes`/`TermSnapshot.modes` lose these numbers — a contract change to record (sibling G4 owns LIMITATIONS.md; handoff flagged) | — |
| O6 | Palette: OSC 4 overrides + OSC 4 query replies (`frame.rs` L42–55, `capture.rs` L73–80, L107–124) | **GAP (sharpest).** termpane neither stores OSC 4 nor answers its queries (`handle_osc`, `grid.rs` L1737–1835, has no `4` arm; unhandled OSC is dropped). **Repro:** `tests/tui_shell/state.rs::live_title_bells_modes_palette` (OSC 4 set → `assert_palette_entry`) and `tests/tui/session.rs:18` (`palette.is_known()`) fail under termpane. Options: (a) upstream OSC 4 support (new external ask), (b) contract change (`Known(empty)` + test updates — needs explicit approval, it is a behavior regression), (c) rejected: adapter-side pre-scan parser (divergent second parser — worse than the gap) | — |
| O7 | Default fg/bg: OSC 10/11 **set** forms (`replay_state.rs` L64–67) | **GAP.** Set forms are explicitly dropped (`grid.rs` L1774–1780); only query forms are answered, from capsule-set (not program-set) colors. Live path already reports `Unsupported` (no live test pins defaults); replay `TermSnapshot.defaults` coverage must be re-verified at swap time | — |
| O8 | Title + bells (`capture.rs` L53–55, `WorkerEventState`) | COVERED with adapter rules: accumulate `PassthroughEvent::TitleChanged/Bell` (`passthrough.rs` L20–29) into worker state; map `TitleChanged("")` → `None` to preserve alacritty `ResetTitle` semantics; `IconNameChanged` (OSC 1 — termpane models it, alacritty folded it into `Title`): map into title as today, or ignore — decide at swap, pin with a test | `drain_passthrough` (`grid.rs` L642) |
| O9 | Clipboard OSC 52 (`replay_screen.rs` L24–30, `SandboxClipboard`) | COVERED with adapter decode: `ClipboardWrite("<sel>;<b64>")` (`passthrough.rs` L34–36); base64-decode via the already-present `base64` dep; selection map `c→Clipboard, p|s→Selection` (matches alacritty 0.26 `term/mod.rs:1712–1713`, verified in registry source) | `drain_passthrough` |
| O10 | Hyperlinks OSC 8 (`replay_state.rs` L78–87) | COVERED (richer: id + uri) | `Cell.hyperlink{uri,…}` (`cell.rs` L84–94, L104–118), `SnapCell.hyperlink_uri`, `hyperlink_target_at_content_row` (`grid.rs` L1170`); keep dedupe-by-uri + 1024 cap |
| O11 | Scrollback text (`replay_state.rs` L68–77) | COVERED | `scrollback_rows_at_offset` (L751), `dump_scrollback_view` (L775), `scrollback_len` (L940); replay limit stays 1000 (`DamageGrid::new(rows, cols, 1000)`). Live alacritty default was 10000 but unobservable — keep either, note the choice |
| O12 | Query replies DA1/DA2, DSR 5n/6n/?6n, DECRQM, kitty `?u`, OSC 10/11 query (today `Event::PtyWrite`/`ColorRequest` → stdin) | COVERED: identical routing shape (worker routes `Reply` bytes to stdin, stashes the rest — reference `session.rs` L855–873) | `PassthroughEvent::Reply` (`passthrough.rs` L71–79); emitters `perform.rs` L380–441, `grid.rs` L1782–1791 |
| O13 | Text-area-size replies CSI 14/16/18t (`capture.rs` L81–94) | **GAP (minor).** termpane never answers them. No test/fixture queries them; impact is limited to programs that block on that answer. Record in LIMITATIONS (handoff) | — |
| O14 | Clipboard **read** answers OSC 52 `?` (today: honestly-empty reply, `capture.rs` L65–72) | COVERED with adapter rule: `ClipboardWrite` payload `"<sel>;?"` → reply the empty-store sequence, preserving today's behavior | `drain_passthrough` + writer |
| O15 | Synchronized output DEC 2026 (today: rejected — `profile.rs` L79–82, `wait_frame` always `Unsupported`, pinned by `tests/tui/input.rs:55` + `:274`) | **IMPROVEMENT available:** termpane tracks it (`in_synchronized_update`, `grid.rs` L1021; `decrqm_status(2026)`). Recommend **two-phase**: swap keeps `wait_frame` fail-closed + rejection (approval stability), follow-up enables + flips those tests | `in_synchronized_update` |
| O16 | Per-cell blink SGR 5/6 (today: dropped — `frame.rs` L162, `profile.rs` L84–88, pinned by `tests/tui/input.rs:284`) | **IMPROVEMENT, approval-affecting:** termpane tracks `slow_blink`/`rapid_blink` per cell (`cell.rs` L170–181, `SnapCellAttrs` L50–53), so `Mods.blink` becomes populated and the `cell_blink` profile rejection **lifts** (the `input.rs:284` test flips to accept). No fixture app emits blink, so no `.snap` churn expected — but any blink-byte test vectors change content | — |
| O17 | Overline SGR 53 | Dropped (no `Mods` field; same class as before, termpane just tracks more than we project). Note in LIMITATIONS (handoff) | — |

### Input encoding — COVERED, adapter-owned on termpane mode getters

| # | Capability (today) | Verdict | termpane path |
|---|---|---|---|
| I1 | Key encoding: kitty `CSI u` vs legacy, app-cursor arrows (`encode_key.rs`) | COVERED. Kitty-active ⟺ `kitty_kb_flags() != 0`; app-cursor ⟺ `application_cursor()`. Parity check at swap: kitty push/pop fixtures must encode identically (termpane answers `?u` queries with `?0u` — programs that *push* unilaterally, the common case, are unaffected) | `kitty_kb_flags` (`grid.rs` L1099), `application_cursor` (L984) |
| I2 | Mouse encoding: SGR/UTF-8/X10 + mode gates 1000/1002/1003 (`encode.rs` L99–206) | COVERED. termpane adds a `Urxvt` (1015) encoding variant (`grid.rs` L93) with no tui-snap counterpart: map to legacy X10 (documented, pinned by test) | `mouse_protocol_mode/encoding` (`grid.rs` L964/L969) |
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
| E1 | argv-verbatim, child-only env + `TERM` default, child cwd, no parent mutation (`builder.rs` L234–249) | COVERED: `SpawnParams` (verbatim argv, overrides-only env, `current_dir`) + adapter-set `TERM`. **Do not adopt** `SessionOptions.colorterm` (`"truecolor"` default) — tui-snap exports no `COLORTERM` today and `SpawnParams` has no unset, so the raw-`spawn_pty` architecture (not `PtySession::spawn`) is required to preserve this |
| E2 | Non-Unix builds (Windows stubs: `signal`, `process_exists`, guardian) | **GAP / scope decision.** `termpane` `process`/`pty` are Unix-only with `compile_error!` on other targets (`lib.rs`). `portable-pty`/`alacritty_terminal` build on Windows today. The swap must Unix-gate the `pty` feature (target-gated optional dep + `cfg(unix)`; `dep:` refs to target-specific optional deps resolve to nothing off-target — verify at swap time) and keep/extend the non-Unix stubs. If Windows PTY support is a requirement, that is a second upstream ask |
| E3 | `MIN_COLS = 2` (alacritty floor, `limits.rs` L11) vs termpane `MIN_COLS = 1` | Relaxation opportunity: adopt 1 (matches the replay path's 1..=1000 and static 1×1 screens). Small behavior expansion; update `Tui::size`/`resize` validation + tests |

## 3. Swap procedure (runs only after §0 clears)

Preconditions: a crates.io `termpane` release `=X.Y.Z` whose API
covers every COVERED row above (re-run §2 against the release — the
map is pinned to `dad8389`, not to "PR #25, whatever merged").
`cargo publish` rights are upstream's; we only consume.

1. **Manifests.**
   - Root `Cargo.toml` `[workspace.dependencies]`: delete the three
     entries + temp comment; add `termpane = "=X.Y.Z"` (exact pin, per
     repo policy "real compatible released versions").
   - `crates/tuiscotti-runtime/Cargo.toml`: `pty =
     ["dep:termpane"]`; replace the three optional deps with a
     Unix-gated `termpane = { workspace = true, optional = true,
     features = ["pty"] }` under `[target.'cfg(unix)'.dependencies]`
     (E2). Verify off-Unix `cargo check` still passes via stubs.
   - **Forbidden in this step:** `git =`, `path =`, `[patch]`,
     version `"*"`, or a version range — see §7.
2. **Enforcement flip.** `crates/xtask/src/deps.rs`: delete
   `TEMP_HOLDER` and the `TEMP-ALLOW` branch so any direct occurrence
   of the three names fails in every manifest. `BANNED` stays.
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
   I1–I4 getters); `exit.rs` (`From<termpane::process::ExitStatus>`);
   `input_types.rs` (drop `libc`, delete `Signal::number` or map to
   `process::SIG*`); `session_input.rs` (`signal()` → `process::signal`,
   `Custom(n)` passes through); `exit.rs` (`process_exists` →
   `pid_alive`, O9 note); `limits.rs` (`MIN_COLS` 2→1 per E3; drop
   `PTY_LIFECYCLE` — the guard moves inside termpane); `mod.rs` docs.
   Keep: `session.rs`, `session_teardown.rs`, `shared.rs`, `error.rs`,
   thread architecture, timing constants, diagnostic strings.
4. **Replay rewire** (`src/tui_shell/replay_*.rs`): fresh
   `DamageGrid::new(rows, cols, 1000)` + `process` + `drain_passthrough`
   + `dump`; same projections as step 3 (R1–R2, O9–O11). Keep
   `Recording`/`Replayed`/`MAX_REPLAY_BYTES`/caps byte-identical.
5. **Test helper**: `tests/tui_shell/replay.rs::capture_raw` drops
   `use portable_pty` → same shape on `termpane::pty` (`spawn_pty` +
   reader thread + bounded drain).
6. **Contract-change tests** (behavior deltas needing explicit sign-off
   at swap time, not silent fixes): `state.rs::live_title_bells_modes_palette`
   + `session.rs:18` (O6 palette gap); `input.rs:284` (O16 blink
   rejection lifts); `input.rs:55` + `:274` stay red→green *unchanged*
   (O15 two-phase: still `Unsupported`); add pins for O8 (`""`→None,
   OSC 1 rule), I2 (Urxvt→X10), O9 (EPE
...[truncated 4305 chars]