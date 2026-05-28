# OpenJill Rust Port Plan

This repository currently contains the upstream OpenJill Java/Maven source. The
Rust port should keep that source as reference material while introducing a
Cargo workspace beside it. The first milestone is a data-faithful Jill of the
Jungle episode 1 runtime; trilogy support should come from generalizing the file
loaders after episode 1 works.

## Goals

- Run from the original DOS game data supplied by the user.
- Do not commit proprietary Jill game data into this repository.
- Port the OpenJill parts needed for a complete Jill of the Jungle episode 1
  playthrough first.
- Keep file formats, level data, entity behavior, rendering, and input isolated
  into small crates so the engine can be tested without a windowing backend.
- Use a modern Rust runtime stack: `wgpu` for rendering, `winit` for windowing
  and input, and `rodio` for audio.
- Preserve the Java OpenJill modules as reference until each area has Rust
  parity and tests.

## Baseline Delivery Target

The baseline project target is a running, playable Jill of the Jungle episode 1
using the original DOS game data wherever possible. "Running" means:

- The application starts from a command line and opens a native `winit` window.
- The app verifies a user-provided DOS data directory before trying to play.
- The renderer presents a 320x200 game framebuffer through `wgpu`, scaled for a
  modern window.
- The game loads `JILL.DMA`, `JILL1.SHA`, `JILL1.VCL`, `JILL1.CFG`, `INTRO.JN1`,
  `MAP.JN1`, and episode 1 `*.JN1` level/save data from disk.
- The user can navigate intro/start menu flow, enter the map, enter levels,
  control Jill, complete level transitions, and finish episode 1.
- Saves and high scores persist outside the repository without modifying
  committed files.
- Audio is routed through `rodio`. If exact original audio decoding is not yet
  understood, the audio layer must still expose the final sound-event API and
  clearly document which original sound/music data remains unimplemented.

VGA is the required first visual mode for the complete episode 1 target. CGA and
EGA support should be added only after the VGA path can complete episode 1.

## Agent Work Protocol

This plan is intended to be worked by individual coding agents. Each top-level
phase below is owned by one phase agent.

Strict rule: before implementing any top-level phase, the assigned agent must
derive a decision-complete subplan for that phase. Do not start code changes for
a phase until its subplan exists.

Each phase subplan must state:

- Which OpenJill Java modules and Rust crates were inspected.
- Which Rust crates/files the phase expects to touch.
- Which original data files are needed for manual or gated integration checks.
- Which public interfaces, commands, data types, or crate dependencies will be
  added or changed.
- Which tests and acceptance checks prove the phase is complete.
- Known risks, unresolved parity questions, and handoff notes for later agents.

Store subplans as `docs/port/NN-<phase-slug>.md` once the Rust workspace exists.
Until then, a phase agent may attach the subplan to its task notes, but the
first workspace/foundation agent must create the docs location and move any
existing subplans there. Agents must not expand scope beyond their phase without
updating the subplan.

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
  openjill-sha-extract/   openjill-sha-edit/
  openjill-jn-extract/    openjill-jn-view/
  openjill-dma-extract/
  openjill-vcl-extract/
  openjill-cfg-extract/   openjill-cfg-view/
  openjill-ui-demo/
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
  Present through `wgpu` and receive window/input state from `winit`.
- `openjill-audio`: `rodio` output, sound-event routing, and eventually original
  sound/music decoding once the DOS data format is understood.
- `openjill-cli`: user entrypoint. Commands should include `run`,
  `data verify`, and eventually `dump`.
- `tools/openjill-*-extract` / `tools/openjill-*-{edit,view}`: per-format
  developer tools that replace the Java extractor modules. The `*-extract`
  binaries dump SHA atlases, JN maps, DMA tables, VCL text, and CFG
  scores/saves; `sha-edit`, `jn-view`, and `cfg-view` are read-only egui
  viewers built on the shared `openjill-ui` widgets and `openjill-export`
  converters.

Use dependency direction:

```text
openjill-cli -> openjill-game -> openjill-core -> openjill-data
                         \-> openjill-render
                         \-> openjill-audio
tools/openjill-*-extract -> openjill-export -> openjill-data
tools/openjill-{sha-edit,jn-view,cfg-view} -> openjill-ui + openjill-export
```

`openjill-data` must not depend on rendering or gameplay. `openjill-core` should
be testable without OS windows, audio devices, or real copyrighted assets.

## Required Runtime Interfaces

The first runnable CLI should expose these commands:

```text
openjill-rs run --data-dir <path>
openjill-rs data verify --data-dir <path>
openjill-rs dump <kind> --data-dir <path>
```

`OPENJILL_DATA_DIR` should be accepted as an environment fallback when
`--data-dir` is omitted. CLI errors must name missing files and invalid paths
directly.

The gameplay runtime should expose these boundaries between crates:

- `openjill-data`: parsed file structs plus case-insensitive data-directory file
  resolution.
- `openjill-core`: deterministic game state, input commands, update/tick
  methods, render commands, and sound events.
- `openjill-render`: uploadable palettes/textures or framebuffer data plus a
  `wgpu` presenter.
- `openjill-audio`: sound-event consumer backed by `rodio`.
- `openjill-game`: orchestration that connects data, core, render, audio, and
  `winit` event handling.

## File Format Port Notes

For full byte-level specs, including MAC demo macros, JN object/string-stack
details, SHA palette-override tiles, and CFG round-trip rules, see the
ModdingWiki-derived reference at
[`docs/port/00-format-reference.md`](docs/port/00-format-reference.md). The
notes below are the port-specific deltas (Java source pointers, parser
expectations) layered on top of that reference.

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

Each top-level step in this section must produce its own subplan before
implementation. The subplan should be detailed enough that another coding agent
could complete the step without making architectural decisions.

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

## Top-Level Agent Phases

### 1. Workspace/Foundation Agent

Derive a subplan, then create the Cargo workspace, crate skeletons, shared
lint/test commands, data-directory ignore rules, docs location, and initial CLI
shape. This phase owns root workspace structure and should not implement real
parsers or gameplay.

Acceptance:

- Workspace builds with empty/minimal crates.
- `data/original/` and `data/extracted/` are ignored.
- `docs/port/` exists for phase subplans.
- CLI command names are stubbed without pretending gameplay works.

### 2. Data Parser Agent

Derive a subplan, then implement `openjill-data` parsing for `DMA`, `VCL`,
`CFG`, `SHA`, and `JN`. Use synthetic fixtures for committed tests and optional
real-data checks gated by `OPENJILL_DATA_DIR`.

Acceptance:

- Parsers return structured data and useful errors.
- Parser tests pass without proprietary data.
- Real-data checks can validate episode 1 files when `OPENJILL_DATA_DIR` points
  at a DOS Jill directory.

### 3. Asset/Debug Tool Agent

Derive a subplan, then implement `data verify` and dump tooling for SHA atlases,
VCL text, DMA tables, and JN map/screen metadata. This phase exists to make
later renderer and gameplay work inspectable.

Acceptance:

- `openjill-rs data verify --data-dir <path>` reports present/missing files,
  sizes, and checksums.
- Dump commands write only to user-selected output paths or ignored
  `data/extracted/` paths.
- Dumps do not need to be committed.

### 4. Render/Input Agent

Derive a subplan, then implement the `winit` window loop, `wgpu` presentation,
320x200 indexed framebuffer, VGA palette expansion, sprite/tile blitting, text
drawing primitives, scaling, and keyboard input mapping.

Acceptance:

- A window opens and presents a nonblank 320x200 framebuffer via `wgpu`.
- Render code can draw parsed SHA images/tiles from original data.
- Input is converted into engine commands without embedding gameplay rules in
  the renderer.

### 5. Core Runtime Agent

Derive a subplan, then implement deterministic runtime state: asset cache,
screen/level configuration, message dispatcher, start menu flow, map loading,
level loading, and level transitions. This phase connects parsed data to
renderable/gameplay state but should keep entity behavior minimal.

Acceptance:

- Intro/start/menu/map/level screens can be loaded from original episode 1 data.
- Runtime state can advance by fixed ticks.
- Screen transitions match the OpenJill episode 1 flow closely enough for
  gameplay agents to attach behavior.

### 6. Gameplay Agent

Derive a subplan, then port player movement, collision, scrolling, status bar,
inventory, pickups, keys/doors, hazards, enemies, moving platforms, projectiles,
and level completion rules needed to finish episode 1.

Acceptance:

- Jill can be controlled through episode 1 levels.
- Required objects/background interactions are implemented for a complete
  episode 1 playthrough.
- Deterministic gameplay tests cover movement, collision, inventory, damage,
  pickups, and level exits where practical.

### 7. Save/Config Agent

Derive a subplan, then implement compatible local save slot, high score, and
config behavior around `JILL1.CFG` and the `JN1` prefix. Writes must go to a
user-writable runtime directory, not committed data directories.

Acceptance:

- Existing high scores and save names can be read.
- Save/load works across app restarts.
- High score updates persist locally.
- Missing or read-only original data does not crash the app.

### 8. Audio Agent

Derive a subplan, then implement `rodio` output and a stable sound-event API.
Investigate original VCL sound data and wire any understood music/sfx. Unknown
or unsupported sounds should fail silently at playback time but remain visible in
debug logs.

Acceptance:

- Runtime can emit sound events without coupling to `rodio`.
- Audio backend initializes, plays supported sounds, and shuts down cleanly.
- Unsupported original audio data is documented with next steps.

### 9. Integration/Parity Agent

Derive a subplan, then run the whole game, compare behavior against OpenJill
references where practical, document remaining parity gaps, and produce user run
instructions.

Acceptance:

- Full episode 1 can be completed from original data.
- `cargo test` passes without proprietary data.
- Optional real-data checks pass when `OPENJILL_DATA_DIR` is set.
- Run instructions describe data placement, CLI usage, controls, and known
  limitations.

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

## End Of Day Acceptance Checklist

- `cargo test` passes without proprietary data.
- Optional real-data tests/checks pass when `OPENJILL_DATA_DIR` points at a
  valid Jill of the Jungle episode 1 DOS data directory.
- `openjill-rs data verify --data-dir <path>` reports all required episode 1
  files as present.
- `openjill-rs run --data-dir <path>` opens a native window through `winit` and
  presents through `wgpu`.
- The app can load original episode 1 menu, map, and level data.
- Jill is controllable and can complete the episode 1 progression.
- Saves and high scores persist in a local writable runtime location.
- Missing or invalid original data produces actionable error messages.
- Audio events are routed through `rodio`; any unsupported original sound/music
  data is documented as a known limitation.

## Open Questions

- Should original OpenJill JSON/properties resources be loaded as-is at runtime,
  translated to Rust data files, or compiled into Rust structs?
- How much exact DOS behavior is required for timing, physics, and save format
  compatibility versus matching OpenJill's behavior?
- Which runtime directory convention should be used for writable saves and high
  scores on each platform?
