# C09 Original-Approval Baseline

## Purpose

C09: prove the approved visual contracts are byte-identical through the
migration. This file pins the pre-migration state of every approval and
fixture artifact plus the renderer/profile/font identities that produced
them. Any post-migration diff against these hashes is either an intended
contract change (must be called out) or a regression.

- Branch: `redesign/rust-first-testing-platform`
- Crate version (`Cargo.toml`): `0.2.0`
- Commit SHA: not recorded (baseline captured without git access; re-verify
  commands below re-derive everything from the working tree).

## Scope

35 files hashed across three roots:

| Root | Files |
|---|---|
| `tests/visual/approved` | 24 × `*.frame.json` |
| `tests/fixtures/render-baseline` | 4 × `*.png` + 4 × `*.png.fidelity.json` |
| `tests/fixtures/consumer` | `Cargo.toml`, `Cargo.lock`, `src/main.rs` |

Plus 12 files under `assets/fonts` (8 font binaries + `FONTS.md` + 3 license texts; see font table).

Excluded: `tests/fixtures/consumer/target` is a symlink to an external
build-cache dir (`/Users/donbeave/Library/Caches/mbx/targets/...`), not a
repo artifact; `find -type f` does not descend into it. No approval depends
on it.

## Method

Hashes are SHA-256 over raw file bytes, produced with:

```sh
find tests/visual/approved tests/fixtures/render-baseline tests/fixtures/consumer -type f | sort | xargs shasum -a 256
shasum -a 256 assets/fonts/*
```

(`shasum -a 256` is the macOS equivalent of `sha256sum`.)

## Approval hash table

### `tests/visual/approved` (24 files)

```
e21af0cabf746ef210407b9d26985146bdaf1fda5a70a58e45f3e55845961355  tests/visual/approved/dialog-dark-120x40.frame.json
5fcf058a86d16ae979fe3235e2a9dc498074fa86710f6b45426b1452e3b163f8  tests/visual/approved/dialog-dark-160x50.frame.json
5e08eabace3565718a6e86070358201eaf36d768ed1059f957a7fc135d487f69  tests/visual/approved/dialog-dark-80x24.frame.json
1b280e81326d46a2b2a4fdb4743bb78a6afd1a78c0a199f017088664f88374e7  tests/visual/approved/dialog-light-120x40.frame.json
ff10a360a18630807565834685043f7e4df8eedfa09476231a386fc1d245e273  tests/visual/approved/dialog-light-160x50.frame.json
06eae2167c525377e6c3763c5ddd395bb27f439765a5469d1bba1054d17a40e7  tests/visual/approved/dialog-light-80x24.frame.json
17815f9a0a933dc076e5b1bc21ebe89900c67acf197bb7d64fa70268b2346398  tests/visual/approved/glyphs-dark-120x40.frame.json
21b6341cd1e6e7fca8d667d8221f936c9fb99986eb4d91251f95c205ef1f1a35  tests/visual/approved/glyphs-dark-160x50.frame.json
0adc075257d5fcfb7f083cd40232c3ced22506e9adbcf8cb9ada00464d372699  tests/visual/approved/glyphs-dark-80x24.frame.json
ada68eedcc3908602a39cda2646a781987cb84acf88fd075ca4d9499efbfc427  tests/visual/approved/glyphs-light-120x40.frame.json
d5a043a89967a739461a322dc13c23ad9058ca32d5f769d60760f5869a5ff74e  tests/visual/approved/glyphs-light-160x50.frame.json
b5e06380daa1928ea489eca8874a7e4409afa0b3f716729ee621193cba349f13  tests/visual/approved/glyphs-light-80x24.frame.json
443d6fe82212b02ea03a5d3ec7f625af0d00958e8bd1ac0f1d4dc79d6608511e  tests/visual/approved/home-dark-120x40.frame.json
9209757b3e6559223db336998c370ce191863c54c63c711ac8a68bde4b49d9d3  tests/visual/approved/home-dark-160x50.frame.json
24edd11667dd72c4fd3a904563d02f89283c9a638dafa0820ac3ad2b765328bb  tests/visual/approved/home-dark-80x24.frame.json
32194ac1307ff517400dc8497d157367b526e51c8571202b46e3e7830f67acec  tests/visual/approved/home-light-120x40.frame.json
4a6f132e50f0fc49adb8381a3402bfb5545bb1fb56c1ab5083d12a187d76a97e  tests/visual/approved/home-light-160x50.frame.json
f877fc6b8116c051dda4f3e4e545918bf3ebabdfabde5402d58c078410de016c  tests/visual/approved/home-light-80x24.frame.json
234476891dfe4759b2559f04af31050c02ff36935d2bb7b576ea94b4d641c618  tests/visual/approved/table-dark-120x40.frame.json
e6be135de5de9f774b52cc63d00fd0356625f53f19dd2176a8e7bc7c4e662436  tests/visual/approved/table-dark-160x50.frame.json
8f8873e2bee401611b0969a919ca77c24c712cb38519edff4650f6e73b256801  tests/visual/approved/table-dark-80x24.frame.json
9c960d8e0b0e14a1502145c6e633e504d78eac0def26df490c992db703b8eff0  tests/visual/approved/table-light-120x40.frame.json
cefdd5ae5a92f8ca343db7d3ab54bc6f560d4cd630c4f43c1040fe84ace76167  tests/visual/approved/table-light-160x50.frame.json
26f6f6d829681431801bb57381ff29cd317ebb6fe7ec57d3017948820cb42d47  tests/visual/approved/table-light-80x24.frame.json
```

All 24 carry `"version":3`, matching `FRAME_VERSION = 3` (`src/frame.rs:22`).
Naming grid is complete and regular: 4 views
(dialog/glyphs/home/table) × 2 themes (dark/light) × 3 sizes
(80x24/120x40/160x50) = 24.

### `tests/fixtures/render-baseline` (8 files)

```
1537d7e4b1ce6baf819c92d70b48098cd83498aac841bb68c9322c8b03b44654  tests/fixtures/render-baseline/dialog-light-80x24.png
b9f24a371170f3063c6ad176d1899618e54222d91d4d9e9b4de54cd3e1f97562  tests/fixtures/render-baseline/dialog-light-80x24.png.fidelity.json
e668a6aa65bd015daddbc3404156384aef1be7ab333e0f77fa7fe3d3c2fc0e39  tests/fixtures/render-baseline/home-dark-80x24.png
b9f24a371170f3063c6ad176d1899618e54222d91d4d9e9b4de54cd3e1f97562  tests/fixtures/render-baseline/home-dark-80x24.png.fidelity.json
1b00f13757f442f11fe26fe3c4b85636d841f038bfadea9175d336faf2d70fb5  tests/fixtures/render-baseline/home-light-160x50.png
b9f24a371170f3063c6ad176d1899618e54222d91d4d9e9b4de54cd3e1f97562  tests/fixtures/render-baseline/home-light-160x50.png.fidelity.json
ee8b2b25d2586088a6c0575cd779f6acb8d7333754868fc72705ffe0d58a6b79  tests/fixtures/render-baseline/table-dark-120x40.png
b9f24a371170f3063c6ad176d1899618e54222d91d4d9e9b4de54cd3e1f97562  tests/fixtures/render-baseline/table-dark-120x40.png.fidelity.json
```

Note: all four `.png.fidelity.json` sidecars are byte-identical (same
hash). Content is the no-fallback/no-missing record
(`profile "tuisnap-default"`, `scale 2`, `approximate false`,
empty `faces_fell_back`/`missing`), so all four PNGs rendered fully from
the primary face. Expected, not suspicious — but it means the sidecars
only pin the profile identity, not per-image fidelity.

### `tests/fixtures/consumer` (3 files)

```
e3dc0b0b4c9b0f010e180561a999d1670527ecd18e85204264d8887a08300d36  tests/fixtures/consumer/Cargo.lock
0beab98cafe0bb314d6438053da25837e31658a4f41772c88722c0337ab7614d  tests/fixtures/consumer/Cargo.toml
ea6925b580a35fc2eba9ef040c84e6a3dc6df1a828fdf6c4de27ca31dc1aa53e  tests/fixtures/consumer/src/main.rs
```

`Cargo.lock` (58317 bytes) is `@generated by Cargo`, not hand-reviewed;
it pins the consumer-proof dependency closure. `Cargo.toml` declares the
`tuisnap-consumer-proof` package with a path dependency on the workspace
root; `src/main.rs` asserts the public API surface
(`Provenance::now`, `ansi::replay_raw`, `pty::Session::spawn`,
`termlens::Screen`) compiles for an ordinary consumer.

## Font identity table

SHA-256 of each file under `assets/fonts`:

```
9b55ade625f2d3f2a273ed16d9db2d924ad6236222920f9fd1324941cdc3c712  assets/fonts/DejaVuSansMNerdFontMono-Regular.ttf
cdc28160fe880d0c712241b6776bfb66c503e1430260be9b718afa15dea75e72  assets/fonts/FONTS.md
bfcf9a917276ffc058867d87cbc8a5b2f1ab0f4b710e9170dc02763ccb80bd4b  assets/fonts/JetBrainsMonoNerdFontMono-Bold.ttf
9dba502e00e35209f6ed2a151c7376c051657b067cdebbc6e52d06cb9002cf31  assets/fonts/JetBrainsMonoNerdFontMono-BoldItalic.ttf
31efd6ead98746f5b0afa1ee6dba60267ad48db36428360bee327bec10621f97  assets/fonts/JetBrainsMonoNerdFontMono-Italic.ttf
f2a5ea6cfab397445ffab00c0370927b66d61e560a05db5db271b42006381c1a  assets/fonts/JetBrainsMonoNerdFontMono-Regular.ttf
7a083b136e64d064794c3419751e5c7dd10d2f64c108fe5ba161eae5e5958a93  assets/fonts/LICENSE-DejaVuSansMono.txt
1d361a8f8e8ce6e68457dcd93fb56e162e6baa3bbb7e7573a290d44399f6b57e  assets/fonts/LICENSE-JetBrainsMono.txt
b118dd41337806a5d4797052c77caf3bd096aed783e5eb21b4d11154351e1ac0  assets/fonts/LICENSE-Noto.txt
777bee41f0c6076c00ad919384359a6e396b8822cf9056041fca8fcf2759d897  assets/fonts/NotoSansCJKjp-subset.otf
6f9cc93e71f8676361c5db286368be046e75d42c0b841afbf3f50da6bb0a2b8a  assets/fonts/NotoSansSymbols-subset.ttf
e1d177a40af910100eceb0e825331e55f0cfd005bc0f26087fd4e58fbe60e6c5  assets/fonts/NotoSansSymbols2-subset.ttf
```

Pinned hashes quoted from `src/profile.rs` (verified to match the files
above at baseline time):

```rust
// src/profile.rs:84-85
pub const VENDORED_SYMBOLS2_FONT_SHA256: &str =
    "e1d177a40af910100eceb0e825331e55f0cfd005bc0f26087fd4e58fbe60e6c5";
// src/profile.rs:89-90
pub const VENDORED_SYMBOLS_FONT_SHA256: &str =
    "6f9cc93e71f8676361c5db286368be046e75d42c0b841afbf3f50da6bb0a2b8a";
// src/profile.rs:93-94
pub const VENDORED_CJK_FONT_SHA256: &str =
    "777bee41f0c6076c00ad919384359a6e396b8822cf9056041fca8fcf2759d897";
```

Cross-checks at baseline time (all match):
- Each pinned `VENDORED_*_SHA256` constant equals the on-disk font hash.
- `font_sha256` in the `.png.fidelity.json` sidecars
  (`f2a5ea6c…c1a`) equals `JetBrainsMonoNerdFontMono-Regular.ttf`.
- `DejaVuSansMNerdFontMono-Regular.ttf` is present but NOT referenced by
  `src/profile.rs` (kept for reference only, per the module docs).

## Renderer / profile version constants

No `*_VERSION` constants exist in `src/render.rs` or `src/profile.rs`;
identity is pinned by these values instead (quoted verbatim):

```rust
// src/frame.rs:22 — canonical frame schema version
pub const FRAME_VERSION: u8 = 3;
// src/frame.rs:25 — max viewport dimension on import
pub const MAX_DIM: u16 = 512;
// src/snapshot.rs:101 — diff report cap
pub const MAX_CELL_DIFFS: usize = 100;
```

```rust
// src/profile.rs:162-176 — Profile::default_profile()
name: "tuisnap-default",
font_px: 16.0,
cell_w: 10,
cell_h: 21,
pad: 12,
scale: 2,
default_fg: Rgb::new(0xd0, 0xd0, 0xd0),
default_bg: Rgb::new(0x00, 0x00, 0x00),
font_sha256: font_sha256(VENDORED_FONT),  // = f2a5ea6c…c1a at baseline
font_desc: "vendored JetBrainsMonoNerdFontMono-Regular (SIL OFL 1.1)",
cursor_visible: true,
```

```rust
// src/render.rs:207-219 — Fidelity sidecar schema (field order = JSON order)
pub struct Fidelity {
    pub profile: String,
    pub font_sha256: String,
    pub font_desc: String,
    pub scale: u32,
    pub approximate: bool,
    pub faces_fell_back: Vec<String>,
    pub missing: Vec<MissingGlyph>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub fallback_glyphs: Vec<FallbackGlyph>,
}
```

## Generated vs hand-reviewed (evident cases)

- Generated: `tests/fixtures/consumer/Cargo.lock` (says so in its header);
  the 4 `.png` files + 4 `.fidelity.json` sidecars (renderer output);
  the 24 `.frame.json` approvals (single-line serialized `Frame`s,
  `"version":3` — machine-serialized shape, approved by review rather
  than authored by hand).
- Hand-authored: `tests/fixtures/consumer/Cargo.toml`,
  `tests/fixtures/consumer/src/main.rs`, `assets/fonts/FONTS.md`,
  license texts.

## Verification command block

Re-derive every hash in this file from the working tree (macOS; on Linux
replace `shasum -a 256` with `sha256sum`):

```sh
# 1. Approval + fixture hashes (expect the 35 lines in the tables above)
find tests/visual/approved tests/fixtures/render-baseline tests/fixtures/consumer -type f | sort | xargs shasum -a 256

# 2. Font asset hashes (expect the 12 lines in the font table)
shasum -a 256 assets/fonts/*

# 3. Pinned-constant cross-checks (expect no output = match)
grep -q 'e1d177a40af910100eceb0e825331e55f0cfd005bc0f26087fd4e58fbe60e6c5' src/profile.rs \
  && grep -q '6f9cc93e71f8676361c5db286368be046e75d42c0b841afbf3f50da6bb0a2b8a' src/profile.rs \
  && grep -q '777bee41f0c6076c00ad919384359a6e396b8822cf9056041fca8fcf2759d897' src/profile.rs \
  && grep -q 'pub const FRAME_VERSION: u8 = 3;' src/frame.rs \
  && echo PINNED-CONSTANTS-OK

# 4. Sidecar identity check (expect f2a5ea6c…c1a in all four sidecars)
grep -h font_sha256 tests/fixtures/render-baseline/*.fidelity.json | sort -u
```

## Anomalies

None. All three roots exist; all 35 files + 12 font assets read
successfully; no missing dirs, no unreadable files. Two observations
(recorded, not anomalies):

1. The four fidelity sidecars are byte-identical (all frames fully
   covered by the primary face) — they pin profile identity only.
2. `tests/fixtures/consumer/target` is a symlink to an external
   build cache, excluded from hashing as a non-repo artifact.
