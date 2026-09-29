# CLI reference (`tuisnap`)

Transcribed from the implemented Clap grammar (`tuisnap --help`,
per-command `--help`) at head `0f14262`, verified by running the
binary. `crates/tuiscotti-cli/tests/readme_lock.rs` pins every
subcommand and flag below against live `--help` output; removed
commands (`check`, `run`) are pinned as exit-2 usage errors.

Global shape: `tuisnap <COMMAND>`. No global flags — `--machine` is
pre-scanned out of argv before Clap sees it, so usage text stays
exactly `Usage: tuisnap <COMMAND>`.

Exit statuses: `0` ok; `2` CLI usage error; `3` op error
(`proto::EXIT_OP_ERROR`); `4` verification disagreement
(`proto::EXIT_VERIFY_FAIL`). `capture`/`record` preserve the child's
exit code instead.

## Commands

```text
tuisnap init --dir .                  # scaffold tui-snap.toml + nextest config + example
tuisnap doctor                         # toolchain / fonts / profile / env report
tuisnap schema                         # print the op-protocol JSON schema
tuisnap capture --out shots/home -- ./my-tui --flag
tuisnap inspect --dir shots/home      # offline view; never executes
tuisnap render --input shot.frame.json --format png --out shot
tuisnap diff --expected a.png --actual b.png
tuisnap review --dir verdicts          # list verdicts; fails on any fail
tuisnap accept --store shots home      # approve one snapshot (explicit, per-name)
tuisnap report --dir verdicts --out report.html
tuisnap import --dir frozen           # read-only frozen-tree import
tuisnap session start --name demo -- ./my-tui
tuisnap record --out trace.jsonl -- ./my-tui
tuisnap trace --input trace.jsonl
tuisnap --machine < ops.jsonl         # typed op protocol over stdio
```

### `init [--dir <DIR>] [--force]`

Scaffolds `tui-snap.toml`, `.config/nextest.toml`, and
`tests/visual.rs` under `--dir` (default `.`). Refuses to overwrite
without `--force`. Prints the config-responsibility contract:

- `tui-snap.toml` — capture + assertion policy. Owned by tui-snap;
  read by tests via the Rust API.
- `.config/nextest.toml` — scheduling only. Owned by cargo-nextest;
  tui-snap never parses it.
- Insta config — snapshot review behaviour. Owned by Insta; tui-snap
  honours it and never auto-accepts in CI.

### `doctor`

Prints `tuisnap <version>`, `protocol v<version>`, then `[toolchain]`
(rustc/cargo/nextest probes), `[fonts]` (regular SHA-256, fallback
face count), `[profile]` (cell geometry, font px, scale, pad),
`[platform]` (os, pty availability), `[env]` (`TERM`, `CI`,
`NEXTEST_PROFILE`, `TUISNAP_RUNTIME_DIR`, `TUISNAP_EVIDENCE_DIR`,
`TUISNAP_SNAPSHOT_DIR`).

### `schema`

Prints the JSON Schema for op-protocol v1 (`type`-tagged ops:
`spawn`, `stdin`, `observe`, `snapshot`, `screenshot`, `wait`,
`exit`, `assert`, `render`, `diff`, `session-start`,
`session-stop`, `session-list`, `version`, `capabilities`).

### `capture --out <OUT> [--timeout-ms <N>] [-- <ARGV>...]`

Runs the command, collects artifacts under `--out` (`manifest.json`,
`stdout.bin`, `stderr.bin`). Default timeout 60 000 ms. Preserves
the child's exit code.

### `inspect --dir <DIR>`

Offline artifact listing (`manifest.json` summary + file sizes).
Never executes anything in the directory.

### `render --input <INPUT> [--format <F>]... [--out <PREFIX>] [--font-file <TTF>]`

Renders a canonical `frame.json` to offline artifacts. Repeatable
`--format` (`png`, `svg`, …); `--out` is the output prefix
(default `shot`); `--font-file` overrides the primary face (hash
recorded in the profile; the fallback chain still applies on top).
Offline re-renders are byte-identical (pinned by tests). Writes
`<prefix>.png.fidelity.json` next to every PNG.

### `diff --expected <PNG> --actual <PNG>`

Compares two PNGs by decoded pixels. Exit 0 identical, exit 4 on
any mismatch (one changed channel fails; re-encoding passes).

### `review --dir <DIR>`

Lists offline verdicts. Exit 4 when any verdict fails, 0 on an
empty or all-pass dir.

### `accept [--store <DIR>] <NAME>`

Approves one snapshot: `actual/<NAME>` → `approved/<NAME>`.
Explicit and per-name only — there is no `--all`. Frozen roots
reject. `--store` defaults to `.`.

### `report --dir <DIR> --out <FILE> [--title <T>]`

Writes a standalone offline HTML report from verdicts. Approval
state is never modified.

### `import --dir <DIR>`

Read-only import of a frozen four-artifact tree. Writes nothing;
unsupported trees error.

### `session <start|stop|list|prune|attach>`

Named sessions (versioned endpoints, owner-only runtime dir):

- `start --name <N> [--force] [-- <ARGV>...]` — detached child +
  endpoint file.
- `stop --name <N>` — stop the session, remove its endpoint.
- `list` — sessions with liveness.
- `prune` — drop endpoints whose process already exited.
- `attach --name <N>` — best-effort human view; EOF detaches.

### `record --out <FILE> [--max-events <N>] [--max-bytes <N>] [-- <ARGV>...]`

Runs the command with bounded event recording (defaults: 10 000
events, 10 000 000 bytes). Preserves the child's exit code.

### `trace --input <FILE> [--kind <K>]`

Offline journal view. Never executes the recorded command.

### `--machine` (stdio mode)

`tuisnap --machine < ops.jsonl`: one Op JSON object per stdin line,
one envelope JSON object per stdout line (`{"ok":true,…}` /
`{"ok":false,"error":{"code","message"}}`). Blank lines skipped.
Exit 0 when every op succeeded, else 3. Example:

```sh
echo '{"type":"capabilities"}' | tuisnap --machine
# {"ok":true,"result":{"type":"capabilities","capabilities":{"protocol":"1.0.0",…}}}
```

## Removed commands

`check`, `run`, `digest`, `accept --all`, `report --store`, and
`render` of raw `*.ansi` do not exist. There are no shims: unknown
subcommands exit 2. The op protocol (`--machine`, `proto::execute`)
is the scriptable surface — there are no `tools/*.py` helpers.
