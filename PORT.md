# OpenJill Rust Port Plan

This repository currently contains the upstream OpenJill Java/Maven source. The
Rust port should keep that source as reference material while introducing a
Cargo workspace beside it. The first milestone is a data-faithful Jill of the
Jungle episode 1 runtime; trilogy support should come from generalizing the file
loaders after episode 1 works.

## Goals

- Run from the original DOS game data supplied by the user.
- Do not commit proprietary Jill game data into this repository.
- Port only the OpenJill parts needed for a playable Rust engine first.
- Keep file formats, level data, entity behavior, rendering, and input isolated
  into small crates so the engine can be tested without a windowing backend.
- Preserve the Java OpenJill modules as reference until each area has Rust
  parity and tests.

## Getting The Game Data

Assume the user has a legitimate DOS copy of Jill of the Jungle. The Rust port
should load the data files directly from a local directory, for example:

```text
data/original/jill1/
```

That directory should be ignored by Git. The loader should accept a path from a
CLI flag or config file:

```text
openjill-rs --data-dir data/original/jill1
```

For episode 1, the OpenJill runtime references these required data files:

- `JILL.DMA`: map-code to tile/picture metadata.
- `JILL1.SHA`: shape, tile, font, and picture data.
- `JILL1.VCL`: text entries; sound data exists in the file but OpenJill's Java
  parser only reads text.
- `JILL1.CFG`: high scores, save names, display/audio setup.
- `INTRO.JN1`: intro, story, ordering, credit, noise-maker, and start-menu
  screens.
- `MAP.JN1`: episode 1 world map.
- Level files addressed through the `JN1` save/level prefix. The Java code
  treats this prefix as the base for saved games and level transitions, so the
  Rust loader should discover matching `*.JN1` files case-insensitively in the
  data directory.

Recommended data handling:

- Add `data/original/` and `data/extracted/` to `.gitignore`.
- Keep exact bytes from the DOS install in `data/original/`.
- Generate any debug dumps, PNG atlases, JSON snapshots, or golden fixtures
  under `data/extracted/`; only commit tiny synthetic fixtures created by tests.
- Normalize file lookup to case-insensitive matching, because DOS media and
  modern filesystems disagree on case.
- Add a `data verify` command that reports present/missing files and prints
  sizes/checksums without hard-coding copyrighted bytes.

## OpenJill Source Areas

Required runtime references:

- `abstractfile` and `abstractfile-api`: little-endian byte reader abstraction.
  In Rust this becomes `std::io::{Read, Seek}` helpers plus parser error types.
- `dma-file` and `dma-file-api`: `JILL.DMA` parser. Required.
- `sha-file` and `sha-file-api`: `*.SHA` parser and color-map handling.
  Required for all graphics.
- `jn-file` and `jn-file-api`: `*.JN1` map, object, save-data, and string-stack
  parser. Required for maps, screens, and levels.
- `vcl-file` and `vcl-file-api`: `*.VCL` text parser. Required for messages and
  menus; sound can be postponed.
- `cfg-file` and `cfg-file-api`: `*.CFG` high-score/save/config parser.
  Required for menus and save-game compatibility.
- `openjill-core-api`: core interfaces, constants, message types, tile manager
  contracts, screen types, and picture helpers. Required conceptually, but in
  Rust it should become concrete modules instead of Java-style API crates.
- `openjill-core`: startup flow, level configuration, caches, menus, status bar,
  message boxes, screen handlers, level transitions, and message dispatcher.
  Required.
- `open-jill-object-background`: the fuller object/background behavior set used
  by the playable game. Required for gameplay parity.
- `OpenJill/src/main/resources`: runtime JSON and properties configuration for
  menus, object factories, status bar, control area, messages, and mappings.
  Required as data to translate or load.

Reference-only or lower-priority areas:

- `simplegame`: Java Swing game loop, keyboard, framebuffer, and window glue.
  Use it to understand timing (`55ms`, `320x200`) and frame presentation, but
  replace it with a Rust backend.
- `jn-file-extractor`, `dma-file-extractor`, `sha-file-extractor`: useful
  documentation and debugging references. Reimplement as optional Rust CLI
  commands after the parsers exist.
- `sha-file-edit`: deprecated upstream; do not port.
- `openjill-object-manager`, `openjill-background-manager`,
  `openjill-cache-manager`: older/smaller manager variants. Prefer
  `open-jill-object-background` plus `openjill-core`, but keep these as
  reference for simpler behavior and cache shape.

## Cargo Workspace Shape

Create a root `Cargo.toml` workspace:

```text
Cargo.toml
crates/
  openjill-data/
  openjill-core/
  openjill-game/
  openjill-render/
  openjill-audio/
  openjill-cli/
tools/
  openjill-dump/
```

Suggested crate responsibilities:

- `openjill-data`: binary parsers and data models for `DMA`, `SHA`, `JN`, `VCL`,
  and `CFG`; case-insensitive file resolution; checksums; synthetic fixture
  tests. This replaces `abstractfile-*`, `*-file`, and `*-file-api`.
- `openjill-core`: pure game state and deterministic systems: screen/level
  configuration, message dispatcher, tile lookup, collision helpers, entity
  traits, object/background behavior, inventory, score/lives, save/load model.
  This maps from `openjill-core-api`, most of `openjill-core`, and
  `open-jill-object-background`.
- `openjill-game`: application orchestration: asset cache, current screen,
  level transitions, startup menu flow, save-game selection, and the main tick.
  This maps from `JillMain`, `Abstract*Level`, screen handlers, and `OpenJill`
  resource wiring.
- `openjill-render`: 320x200 indexed framebuffer, VGA/CGA/EGA palette
  expansion, text drawing, sprite/tile blitting, scaling, and presentation.
  Keep this backend-agnostic at the lower layer; add `pixels`/`winit` or SDL2
  behind a feature.
- `openjill-audio`: placeholder crate at first. Later, parse/play the sound
  portion of `VCL` or bridge to a compatible audio representation.
- `openjill-cli`: user entrypoint. Commands should include `run`,
  `data verify`, and eventually `dump`.
- `tools/openjill-dump`: optional developer tool for dumping JN maps, SHA
  atlases, DMA tables, and VCL text. This replaces Java extractor modules.

Use dependency direction:

```text
openjill-cli -> openjill-game -> openjill-core -> openjill-data
                         \-> openjill-render
                         \-> openjill-audio
tools/openjill-dump -> openjill-data -> openjill-render
```

`openjill-data` must not depend on rendering or gameplay. `openjill-core` should
be testable without OS windows, audio devices, or real copyrighted assets.

## File Format Port Notes

`DMA`:

- Java source: `dma-file/src/main/java/org/jill/dma`.
- Entry layout: `u16le map_code`, `u8 tile`, `u8 tileset_with_flags`,
  `u16le flags`, `u8 name_len`, followed by `name_len` bytes.
- Java masks the tileset with `0x3f`.

`SHA`:

- Java source: `sha-file/src/main/java/org/jill/sha`.
- Header contains 128 `u32le` tileset offsets followed by 128 `u16le` sizes.
- Tileset records contain shape count, rotation count, CGA/EGA/VGA lengths,
  color-bit depth, flags, optional color map, then image records.
- Image record starts with `u8 width`, `u8 height`, `u8 type`, then row-major
  pixel/index data.
- Preserve indexed data and palette mapping in data structures; convert to RGBA
  only at render boundaries.

`JN`:

- Java source: `jn-file/src/main/java/org/jill/jn`.
- File order is background layer, object count, object records, save data, then
  string stack. Objects with nonzero string pointers are associated with
  string-stack entries as they are read.
- Treat map/screen files and save-derived level data through the same parser.

`VCL`:

- Java source: `vcl-file/src/main/java/org/jill/vcl`.
- Java skips the first 400 bytes of sound entries, then reads 40 `u32le` text
  offsets and 40 `u16le` text lengths.
- Start with text only. Leave sound decoding behind a tracked TODO.

`CFG`:

- Java source: `cfg-file/src/main/java/org/jill/cfg`.
- File size is expected to be 254 bytes.
- Contains 10 high-score names of 10 bytes, a 20-byte hole, 10 signed `i32le`
  scores, 6 save names of 12 bytes, then joystick/display/music/sound config.
- Save slots use the provided prefix, `JN1` for episode 1.

## Port Order

1. Create the Cargo workspace and crate skeletons.
2. Implement `openjill-data` byte-reader helpers and parsers for `DMA`, `VCL`,
   and `CFG`; these are small and make good first tests.
3. Implement `SHA` parsing and palette-preserving image extraction.
4. Implement a dump tool that can emit a SHA atlas and VCL text listing from a
   local DOS data directory.
5. Implement `JN` parsing, then dump `INTRO.JN1` and `MAP.JN1` as debug JSON or
   text snapshots.
6. Build a minimal `openjill-render` framebuffer that can draw SHA tiles/images
   at 320x200 and scale to a window.
7. Port menu/start screen flow from `openjill-core` and `OpenJill` resources.
8. Port map/level loading and static background rendering.
9. Port player movement, collision, screen scrolling, status bar, inventory, and
   level transitions.
10. Port object/background managers in gameplay clusters: pickups/score, keys
    and doors, hazards, simple enemies, moving platforms, projectiles, bosses or
    special cases.
11. Add save/load and high-score writing once reading is proven.
12. Add audio after the visual/gameplay loop is stable.

## Testing Strategy

- Parser unit tests should use synthetic byte fixtures for edge cases and small
  committed fixtures only.
- Integration tests can run against `OPENJILL_DATA_DIR` when present and should
  be skipped otherwise.
- Add golden metadata tests that assert counts, dimensions, offsets, and names,
  not original copyrighted byte contents.
- Keep deterministic game logic in `openjill-core` so movement and collision can
  be regression-tested without rendering.
- Use dump-tool output during development, but avoid committing dumps generated
  from proprietary game assets.

## Open Questions

- Which window/audio backend should be the default: `winit` plus `pixels`, SDL2,
  or another backend?
- Should original OpenJill JSON/properties resources be loaded as-is at runtime,
  translated to Rust data files, or compiled into Rust structs?
- Should the initial milestone target VGA only, then add CGA/EGA modes after the
  asset pipeline is stable?
- How much exact DOS behavior is required for timing, physics, and save format
  compatibility versus matching OpenJill's behavior?
