# PTY/emulator backend qualification (R04, R05)

Status: qualified 2026-09-28. Probe crates live only under `/tmp/ptyprobe`
(`probe-alacritty`, `probe-vt100`, `probe-ghostty`; target dirs
`/tmp/ptyprobe/target-*`); nothing from the probes is vendored here.

## 1. What the current engine provides

Engine today: `vendor/termlens` 0.9.0 (vendored source copy) over the
unpublished git crate `termpane` v0.1.0 (`src/pty.rs` is a thin adapter).
Inventory from `vendor/termlens/src` + `src/pty.rs`:

| Area | Current provision |
|---|---|
| Cells | text, fg/bg (`Default`/`Indexed`/`Rgb`), bold, dim, italic, underline, reverse, blink, conceal, strikethrough; wide + wide-continuation flags |
| Cursor | (row, col, visible), shape (`Default`/`Block`/`Underline`/`Bar`), blink `Option<bool>` |
| Palette | **none observable** — `Screen` has no palette/default-color accessor; `OSC 4;n;?` queries are only *answered*, never recorded |
| Modes | collapsed `MouseMode` + exact `MouseModes` set, bracketed paste, application cursor, focus 1004, alt-screen; `DECRQM` answered from live state |
| Out-of-band | title, `OSC 52` clipboard, `OSC 8` links, bells, graphics counts, repaint counter, frame timings, text scrollback, full text |
| Queries | app capability probes (DSR/CPR/DA/DECRQM/XTGETTCAP/…) answered in-stream so apps never hang |
| Input | `Key` + modifier chords, app-cursor encoding, literal/paste/bracketed paste, click/drag/scroll with SGR+legacy+UTF8 mouse encodings from live modes, focus out |
| Resize | `set_size`, bounded 2..=1000/axis; **no scrollback reflow** |
| Thread model | emulator behind `Mutex`; background reader thread drains PTY continuously; writer thread for input + query replies; `Drop` kills/reaps (2 s grace); process-global `PTY_LIFECYCLE` mutex serializes open/spawn vs kill/reap (macOS `revoke()` race) |
| Platform | Unix-oriented: no `cfg(windows)` in termlens source; spawn itself delegates to `portable-pty` 0.9 (which has a ConPTY path) |

R04 forbids exactly this shape: a vendored fork plus a git-only backend.
Both must go; see §6.

## 2. Candidate matrix

Versions verified against crates.io API 2026-09-28. `wezterm-term` and
`termpane` return HTTP 404 (unpublished).

| | (1) Ghostty via `libghostty-vt` | (2) `portable-pty` + `wezterm-term` | (3) `portable-pty` + `alacritty_terminal` | (4) `termwiz` / `terminput` |
|---|---|---|---|---|
| Published | `libghostty-vt` **0.2.2** + `-sys` 0.2.2 (2026-09-28) — third party `uzaaft/libghostty-rs`, **not** `ghostty-org` | `portable-pty` **0.9.0** yes; `wezterm-term` **no** (git-only) | `portable-pty` **0.9.0**, `alacritty_terminal` **0.26.0** | `termwiz` **0.23.3**, `terminput` **0.5.15** (`aschey/terminput`) |
| License / MSRV | MIT OR Apache-2.0 / rust 1.90 | MIT / undeclared | MIT + Apache-2.0 / rust 1.85.0 | MIT |
| Rust API | `Terminal::new(Options)`; `vt_write(&[u8])`; `grid_ref(Point)` / `TrackedGridRef`; `RenderState` snapshots + row/cell iterators; `mode(Mode)`; cursor/palette/title/pwd getters; `KeyEncoder`/`MouseEncoder`; `on_*` callbacks (pty-write, bell, title, clipboard, DA) | `Terminal::new(config, writer)` renderer-oriented; rich `TerminalConfiguration` | `Term::new(Config, &dims, listener)`; `vte::ansi::Processor::advance(&mut term, bytes)`; `grid()[Line][Column]`; `grid().cursor`; `mode()`; `colors()`; `cursor_style()`; `Event` sink (`Title`, `Bell`, `ClipboardStore`, `PtyWrite`, `ChildExit`, …) | `termwiz::surface::Surface` is an **output-side** retained screen (`Change` list), not a VT parser; `terminput` is input parse/encode only — neither is an emulator |
| Thread model | handles `!Send + !Sync` **by design** (docs.rs); all ops on one owner thread | wezterm `Terminal` mutex-heavy, renderer-coupled | `Term<VoidListener>` is **`Send + Sync`** (static assert compiled in probe); `EventListener: Send` via `&self` sink | n/a |
| Grid/cursor/palette/mode/scrollback | full: graphemes, styles, wide tags, hyperlink URIs, semantic prompts; cursor pos/visibility/style/blink; full palette + default fg/bg/cursor get/set; `Mode` queries; scrollback rows; selection API | full + bidi/unicode segmentation (`finl_unicode`); strongest text model | grid + alt grid, cursor point, `SHOW_CURSOR` in `TermMode`; `Colors` (269 slots: 0-255, fg, bg, cursor, dims); distinct mouse flags incl. `UTF8_MOUSE`; kitty-keyboard flags; scrollback with **reflow on resize** (`grid/resize.rs`) | n/a |
| Attr gaps | none known (native Ghostty fidelity) | none known | **cell blink dropped** (no `Flags::BLINK`; `Attr::Blink` unhandled — verified in `term/cell.rs`); cursor blink still readable via `cursor_style().blinking`. Tracks hidden, strikeout, all underline styles + underline color | n/a |
| Honesty hook | callbacks surface bell/title/clipboard; unknown-seq reporting unchecked (probe never built) | none | **none** — unknown sequences silently ignored; no `Callbacks::unhandled_*` equivalent | n/a |
| Mouse/keyboard encoding | `KeyEncoder` (kitty flags, app-cursor, modifyOtherKeys) + `MouseEncoder` from live terminal state | full (wezterm input stack) | **consumer-owned**: alacritty parses modes but ships no input encoder; adapter encodes from `TermMode` (same work termlens already does) | `terminput` could serve as the encoder helper |
| Query answering | `on_pty_write` callback channel exists for replies | internal | **adapter-owned**: no query-callback surface; the adapter must tee the byte stream and answer DSR/CPR/DA/DECRQM like termlens `seq.rs` does today | n/a |
| Platforms | Zig cross matrix; Windows MSVC static-link fix landed in 0.2.2 (`ghostty-vt-static.lib`); TurboRepo still carries a `[patch]` for it | macOS/Linux/Windows (ConPTY) | PTY: unix + ConPTY (`NativePtySystem = win::conpty::ConPtySystem`, Win10 1809+); emulator: pure Rust + `tty/{unix,windows}` | macOS/Linux/Windows |
| Build cost | **Zig toolchain required** + build-time `git clone ghostty` (pinned `a887df4`, **155 MiB** into `OUT_DIR` on this machine) + full `zig build -Demit-lib-vt`; exact Zig pin enforced by ghostty | `image`, `terminfo`, `lru`, `wezterm-bidi`, …; ~38 s cold dep build reported | pure cargo, no C deps, no network beyond crates.io; probe lockfiles: **75** packages incl. probe crate (vs 34 for `portable-pty`+`vt100`; ghostty resolved 11 Rust packages but its native build never ran) | 34 direct deps (`image`, `pest`, `terminfo`, `wezterm-*`, …) for no emulator |

## 3. Probe evidence (all in `/tmp`, `CARGO_TARGET_DIR=/tmp/ptyprobe/*`)

1. **`probe-alacritty` — PASS.** `portable-pty` spawns `/bin/echo`,
   bytes feed `Term` through `ansi::Processor::advance`, grid reads back
   `hello-pty`, cursor at line 1 col 0, `TermMode`/`CursorStyle`/`Colors`
   readable. `Term<VoidListener>: Send + Sync` asserted at compile time.
   One behavioral note: the PTY must be **drained before `child.wait()`**
   — `wait` first, then `read_to_end`, returned `[]` on macOS. The adapter
   must keep termlens's drain-before/at-exit discipline (`DRAIN_GRACE`).
2. **`probe-vt100` — PASS** (reference only). Same spawn/drain with
   `vt100` 0.16.2; grid + cursor read back. Confirms the harness shape,
   but `vt100` drops blink/conceal/strikethrough and is strictly weaker
   than (3) — no reason to adopt it.
3. **`probe-ghostty` — BUILD FAILURE (evidence, not a flake).**
   `libghostty-vt-sys` 0.2.2 build script cloned ghostty
   (`a887df42c…`, 155 MiB) then failed:
   `failed to execute zig build: No such file or directory (os error 2)`.
   No Zig toolchain on the machine (only an unpinned `mise` shim), and the
   crate pins an exact Zig minor. So the Ghostty path needs, before any
   fidelity comparison: a vendored/pinned Zig toolchain in CI + dev
   environments, tolerance for a 155 MiB network fetch on every fresh
   build cache, and acceptance of a third-party binding over a `0.1.0-dev`
   C ABI (`lib_version = "0.1.0-dev"` in ghostty `build.zig`).

## 4. Thread-model notes

- The current termlens shape (shared-`Mutex` emulator + reader/writer
  threads, `&self` snapshot / `&mut self` input) ports directly onto
  `alacritty_terminal`: `Term` is `Send + Sync`, `Processor` is a plain
  value owned by the reader side, and `EventListener` is a `&self` sink
  (an `mpsc::Sender`-wrapping listener preserves the `Event` stream
  without locking the grid).
- `libghostty-vt` forbids that shape: `!Send + !Sync` handles plus
  lifetime-bound callbacks (`Terminal<'alloc, 'cb>`) force a dedicated
  owner thread and message passing for every snapshot/input op. Any
  adapter trait must therefore **not** assume a shareable emulator; design
  around an owner-thread + revision-stamped snapshots (§5), which fits
  both backends and directly serves R06 (atomic observation) and R07
  (waits never block unrelated sessions).
- `portable-pty` handles are `Send` (`MasterPty`, `Child`,
  `ChildKiller`); reader/writer are `Send` boxes. Keep the macOS
  `PTY_LIFECYCLE` open-vs-teardown serialization: it guards a kernel
  `revoke()` race, not a termlens bug, and any `portable-pty` consumer on
  macOS needs it.

## 5. Recommendation

- **Default: `portable-pty` 0.9 + `alacritty_terminal` 0.26**
  (feature `pty`). Both published, pure-cargo, headless-proven above,
  ConPTY-capable, no daemon/C/Zig/git inputs. It is the only candidate
  satisfying R04 + R05's "promote only after capability/build/platform
  tests" today.
- **Optional (not yet): `libghostty-vt` 0.2.x** behind a cargo feature,
  promoted only after: (a) reproducible Zig toolchain story,
  (b) source vendoring or `[patch]` replacing the 155 MiB build-time
  clone, (c) full-fidelity corpus run against the §7 differences,
  (d) owner-thread adapter proven. Its upstream is a third-party binding,
  so pin exact versions and re-qualify per release.
- **Rejected:** `wezterm-term` (unpublished → a crate depending on it
  cannot publish; vendoring it contradicts R04); `termwiz` alone and
  `terminput` (no VT emulator — `Surface` is output-side); `vt100`
  (strictly weaker; probe kept as a scratch reference only, not a dep);
  `termpane` (unpublished; removed with the vendor dir).
- **No silent fallback, ever.** Backend choice is a build-time feature
  plus an explicit runtime constructor. Capability differences (§7) are
  reported in `TerminalProfile`/observation metadata, never normalized
  away: a test requiring cell-blink fidelity on the alacritty backend
  must **fail closed** (unsupported-capability error), not pass on a grid
  that silently dropped the attribute. This is R05/R06 as specified.

### Adapter trait shape (target for R04)

```rust
/// Owner-thread–agnostic emulator: implementors may be `!Sync`
/// (Ghostty) — the session below owns the confinement.
trait EmulatorBackend {
    type Snapshot: Clone + Send + Sync; // atomic grid+cursor+palette+modes
    fn feed(&mut self, bytes: &[u8], actions: &mut Vec<BackendAction>);
    fn snapshot(&self) -> Self::Snapshot; // one revision, never torn (R06)
    fn resize(&mut self, cols: u16, rows: u16);
    fn capabilities(&self) -> BackendCaps; // blink-cells? unhandled-seq? reflow? graphics?
}

/// PTY owner: spawn/drain/signal/reap. One impl over `portable-pty`.
trait PtyOwner: Send {
    fn spawn(&mut self, argv: &[String], opts: &SpawnOpts) -> Result<()>;
    fn poll_drain(&mut self) -> Vec<u8>; // non-empty-first, EOF-aware
    fn write_input(&mut self, bytes: &[u8]) -> Result<()>;
    fn resize(&mut self, size: PtySize) -> Result<()>;
    fn try_wait(&mut self) -> Result<Option<ExitStatus>>;
    fn kill(&mut self) -> Result<()>;
}
```

`Session` keeps today's public surface (`send_key`, `click`, waits,
`run_once`) and owns: one emulator on its confined thread (direct
`Mutex<Term>` for alacritty; channel proxy for Ghostty), the
termlens-derived input encoder + query-answerer tee (kept, retargeted at
`TermMode` reads), and the macOS lifecycle guard. Waits evaluate
predicates against revision-stamped `Snapshot`s, so read/compare can
never observe a torn composite.

## 6. `vendor/termlens` removal plan (no shims, no aliases)

1. Land the `portable-pty` + `alacritty_terminal` session + input
   encoder + query tee with the existing `src/pty.rs` public API intact;
   port the PTY test suite to it (fidelity corpus from R05 gates every
   behavioral difference in §7).
2. Delete `vendor/termlens/` entirely.
3. Remove `termlens = { path = "vendor/termlens" }` and the `termpane`
   git dependency from `Cargo.toml`; add `portable-pty` 0.9 and
   `alacritty_terminal` 0.26 under feature `pty`.
4. Rewrite `src/pty.rs` imports off `termlens::` types (`Key`, `Mouse*`,
   `Screen`, `ExitStatus`) onto the new adapter's own types; public
   re-exports of `termlens` types are removed, not aliased.
5. Scrub docs for termlens references; this file replaces them.
6. Verify: `cargo build --no-default-features` stays engine-free (M09);
   full suite + nextest lane green; no `vendor/` dir, no git deps in
   `cargo tree`.

## 7. Recorded capability differences (all survive as explicit reports)

| Capability | termlens today | alacritty default | ghostty optional |
|---|---|---|---|
| Cell blink (SGR 5/6) | tracked | **dropped** — R13 blink assertions fail closed; revisit via byte-tee shadow only if corpus demands | tracked (expected; unproven — probe never built) |
| Conceal / strikeout | tracked | tracked (`HIDDEN`, `STRIKEOUT`) | tracked (expected) |
| Underline style/color | single underline | double/curly/dotted/dashed + color (superset) | superset (expected) |
| Palette/default colors | not observable | observable (`Colors`, 269 slots) — newly assertable (R13) | get/set incl. cursor color |
| Unhandled sequences | tracked (`seq.rs`) | **silent** — adapter must add a `vte`-level tee or report `BackendCaps::unhandled_seq = false` | unchecked |
| App query answering | full (DSR/CPR/DA/DECRQM/…) | **adapter-owned tee** (new code, same contract) | `on_pty_write` channel |
| Scrollback reflow on resize | no | yes | yes (expected) |
| Mouse encodings | SGR + legacy + UTF8 from live modes | modes tracked incl. 1005/1006; encoding adapter-owned | encoder from live state |
| Kitty keyboard/focus/paste/altscreen | tracked | `TermMode` flags | tracked (expected) |
| Graphics payload counting | counted | **absent** — adapter tee or `unsupported` until A07 | kitty-graphics native |
| Repaint/frame-timing counters | yes (DEC 2026) | **adapter-owned** (tee `Begin/EndSynchronizedUpdate`) | via render snapshots |
| Title/clipboard/bells/links | accessors | `Event::{Title, ClipboardStore, Bell, …}`; **links need adapter tracking** (alacritty parses OSC 8 into `Hyperlink`s on cells — readable per cell, no link-list event) | callbacks + cell URIs |
| Scrollback content | text rows | styled grid history (`total_lines` > `screen_lines`) | scrollback rows |
| Thread sharing | `Mutex` + threads | same (`Send + Sync`) | owner thread + channels only |
| Windows | unix-only | ConPTY via `portable-pty`; emulator pure Rust | Zig/MSVC path, least mature |
