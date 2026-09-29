# Limitations

Honest gaps at head `75ff479`. Labels: **tested** (ran green),
**compiles-only**, **not run**, **unsupported** (explicitly out of
scope, fails closed where applicable).

## Platform matrix

| Platform | Status | Evidence |
|---|---|---|
| macOS (aarch64) | tested | Full nextest 519/519 green (~21 s) at head `75ff479`, 2026-09-29. See PERFORMANCE.md. |
| Linux (x86_64) | tested in CI | CI lanes run `ubuntu-24.04` (`platform = "linux-x64"` in `.github/ci/project.toml`); latest `CI / PR` green incl. nextest + doctests. |
| Windows (ConPTY) | compiles-only, not run | `cargo check --target x86_64-pc-windows-gnu --tests`: 0 errors. `portable-pty` 0.9 ships a ConPTY backend, but no Windows test process has ever executed and no Windows CI lane exists. Windows is never green until a CI lane runs it. |

Unix-only code that degrades or compiles out on Windows (paths at
head; tree is moving):

- `tui.rs` — `Signal::number` (`libc::SIG*`) is `#[cfg(unix)]`.
- `tui_shell.rs` — unix guardian/process-group machinery is
  `#[cfg(unix)]`; `#[cfg(not(unix))]` stubs return `None` /
  `Containment::Unsupported` explicitly, not silently.
- `command.rs` — non-unix `classify` maps to `Termination::Exit`,
  never invents a signal.
- `proto.rs` — unix / non-unix session paths.
- `tests/piped.rs` — signal-death test is `#[cfg(unix)]`.
- `tests/cli.rs` — owner-only (0o700) runtime-dir and chmod blocks
  are `#[cfg(unix)]`; skipped, not asserted, on Windows.

For a truthful Windows claim, CI needs a `windows-latest` lane
running the same unit commands. Until then: "compiles for
`x86_64-pc-windows-gnu`; ConPTY path unverified".

## Render fidelity

- Terminal-like, measured fidelity — NOT pixel-identity with any
  terminal emulator; cell data stays authoritative for styles.
- Codepoints no vendored face covers (color emoji, Hangul, JIS
  level-2 kanji) render as deterministic tofu with correct advance
  AND are reported in `<name>.png.fidelity.json` next to every PNG.
  Covered: box drawing, blocks, Braille, Nerd icons, combining
  marks, Geometric Shapes / Misc Symbols / Dingbats subsets, kana +
  JIS X 0208 level-1 kanji + fullwidth forms. See
  `assets/fonts/FONTS.md`.
- Blink phase is frozen visible; slow/rapid rates stay combined.
  Overline is dropped. Raw-ANSI replay cannot observe cursor
  appearance (block/steady cursor) — use PTY captures for cursor
  shape/blink assertions.
- No scrollback reflow on resize (emulator constraint).

## Emulator honesty gaps

- Unknown escape sequences are silently ignored; there is no
  unhandled-sequence reporting surface.
- The adapter owns input encoding and query answering (DSR/CPR/DA/
  DECRQM): alacritty parses modes but ships no encoder or
  query-callback surface, so this behavior is ours to qualify
  (covered by `tool_qualification` + PTY matrix tests).
- Cell blink is dropped by the emulator (cursor blink still
  readable via cursor style).

## Scope

- Foreign-language SDKs (Python/JS/TS clients) are out of scope:
  the transport is Rust-only (`tuiscotti` + `tuiscotti machine`).

## Security posture

- This is a research project: unsafe, breaking changes expected,
  never production-ready.
- Hidden text is conceal, not redaction — canonical JSON retains
  the original symbol. Never capture real secrets
  (`redact_frame`/`redact_screen` exist for scrubbing, but the
  safe default is to not capture secrets at all).
- `inspect`/`trace`/`import`/`review`/`report` never execute
  directory contents; `capture`/`record`/`session start` execute
  only the argv you pass.
- Session runtime dirs are owner-only (unix); `TUISCOTTI_RUNTIME_DIR`
  overrides the location.
