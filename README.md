# openjill-rs

A Rust port of the [OpenJill](http://www.openjill.org) engine for **Jill of the
Jungle** (Epic MegaGames, 1992). It parses the original game data and runs the
game in a `winit` window rendered through `wgpu`.

You must bring your own original game data: **no copyrighted assets are shipped
with this repository.**

## About the game

Jill of the Jungle is a side-scrolling platformer released in 1992 by Epic
MegaGames (now Epic Games) as a three-episode shareware trilogy: *Jill of the
Jungle*, *Jill Goes Underground*, and *Jill Saves the Prince*. It was designed
and programmed by **Tim Sweeney**, Epic's founder and the engineer who went on
to create the Unreal Engine. Ars Technica frames it as
["the last game Tim Sweeney designed"](https://arstechnica.com/gaming/2025/11/revisiting-jill-of-the-jungle-the-last-game-tim-sweeney-designed/)
before he moved to engine and tools work full-time.

It is a small but historically notable title: an early Epic shareware release
(contemporary with ZZT and the studio's first wave of DOS games) that helped
Epic find its footing in the shareware market it would later dominate, and an
early work by an engineer whose technology now underpins a large share of modern
games. It is no *Commander Keen*, but it is far from nothing - and understanding
exactly how its data formats and engine behave is the point of this port.

## Status and scope

- **Episode 1** ("Jill of the Jungle") is playable end to end: start menu, world
  map, levels, saving / loading, high scores, and the original sound effects.
- Episodes 2 and 3 are not yet supported.
- See [`docs/port/`](docs/port/) for the format reference and per-epic porting
  notes, and [Known limitations](#known-limitations) below.

## Requirements

- A recent Rust toolchain (edition 2024; see `rust-version` in `Cargo.toml`).
- A GPU / driver that `wgpu` can use (for the windowed `run`).
- Linux audio build dependency: `libasound2-dev` (and `pkg-config`) for `rodio`
  / `cpal`. Other platforms need no extra audio packages.
- Your original Jill of the Jungle **episode 1** data files (see below).

## Original game data

Point the tools at a directory containing the original episode-1 files
(`JILL.DMA`, `JILL1.SHA`, `JILL1.VCL`, `JILL1.CFG`, `INTRO.JN1`, `MAP.JN1`, and
the level files `1.JN1` ...). The data directory is resolved in this order:

1. `--data-dir <path>` on the command line.
2. the `OPENJILL_DATA_DIR` environment variable.
3. the default `data/original/JILL1` relative to the working directory.

The original data is never copied into or committed to this repository.

## Build

```sh
cargo build --release
```

The CLI binary is `openjill` (crate `openjill-cli`). Examples below use
`cargo run --release --bin openjill -- <args>`; a built binary works the same.

## Verify your data

Check that the required episode-1 files are present and parse:

```sh
cargo run --release --bin openjill -- data verify --data-dir /path/to/JILL1
```

The report lists each required core file as OK or missing and ends with
`ok: true` when all are valid. Level `*.JN1` files are also listed as
informational "discovered" entries, but only the core files determine the
pass/fail result.

## Run

```sh
cargo run --release --bin openjill -- run --data-dir /path/to/JILL1
```

Opens the game window. (With the project [Taskfile](https://taskfile.dev),
`task data:run` does the same against the default data directory.)

## Controls

| Key(s)                 | Action                              |
|------------------------|-------------------------------------|
| Left / Right arrows    | Move                                |
| Up arrow               | Climb / look up                     |
| Down arrow             | Duck                                |
| Space / Shift          | Jump                                |
| Ctrl / Alt             | Throw knife                         |
| Tab / Backspace        | Next / previous inventory item      |
| Escape                 | In a level: open the "really quit?" confirmation. In a menu: dismiss it, or quit from the title menu. |
| `S` / `R`              | Save / restore game                 |
| `N`                    | Toggle sound on / off               |
| `T`                    | Toggle turtle (slow-motion) mode    |
| `Q`                    | Quit                                |

## Title-screen cheats

Two hidden commands from the original game work on the **title menu**:

| Key      | Action                            |
|----------|-----------------------------------|
| `Ctrl+P` | Play the intro level              |
| `Ctrl+E` | Open the in-game level editor     |

### Level editor (work in progress)

`Ctrl+E` opens a port of the game's hidden built-in level editor (rediscovered
by Malvineous; see the [ModdingWiki](https://moddingwiki.shikadi.net/wiki/Using_the_official_Jill_of_the_Jungle_level_editor)).
It currently opens on a blank board with these controls:

| Key(s)          | Action                                  |
|-----------------|-----------------------------------------|
| Arrow keys      | Move the tile cursor                    |
| Tab / Backspace | Next / previous tile in the palette     |
| Space / Shift   | Paint the selected tile at the cursor   |
| `K`             | Pick the tile under the cursor          |
| `H`             | Flood-fill the cursor row               |
| `Z` / `N`       | Clear to a new blank board              |
| `S` / `L`       | Save / load a board (type a file name, Space confirms, Escape cancels) |
| `O`             | Enter object mode (Escape leaves it)    |
| Escape          | Return to the title menu                |

In **object mode** the cursor + arrows move over the object layer: `A` adds an
object (type its name, Space confirms), `D` deletes the object under the cursor,
`K` selects it, `M` moves the selected object to the cursor, `P` pastes a copy
of it at the cursor.

Saved boards go to a writable per-user directory (never the read-only original
data). A few original editor commands (field-level object modify, load tile by
name) are still in progress - see issue #210.

## Saving and high scores

Saves, high scores, and the working config are written to a per-user, writable
directory (the original data directory stays read-only). The location resolves
as:

1. `OPENJILL_STATE_DIR` (explicit override), else
2. the platform data dir under `openjill/` (e.g. `~/.local/share/openjill/` on
   Linux, `%APPDATA%\openjill\` on Windows), else
3. a temporary directory as a last resort,

with a per-episode subdirectory underneath.

## Audio

Sound effects are decoded from the original `*.VCL` data (8-bit PCM) and played
through `rodio`. Press `N` to mute / unmute. If no audio device is available the
game runs silently. Background music (`*.DDT`) is not played; see below.

## Known limitations

- **Episodes 2 and 3** are not yet supported (episode 1 only).
- **Background music** (`*.DDT`, Adlib / OPL2) is not played - a future epic.
- The **sound-effect-to-event mapping** is only partial: the player cues
  (jump, fire, hurt, die) are mapped; the rest await reverse-engineering of the
  original executable (tracked in issue #209).
- There is **no automated full playthrough**; level-completion correctness is
  validated by manual playtest plus an integration smoke test.
- Parity with the Java OpenJill engine is argued **structurally** (round-trip
  and real-data parser tests, ported reference constants) rather than by a live
  side-by-side run.

## Troubleshooting

- **`data verify` reports a missing file**: confirm `--data-dir` points at the
  episode-1 folder and the listed file exists (filenames are matched
  case-insensitively).
- **No sound**: ensure an audio device is available; on Linux install
  `libasound2-dev` before building. Check that sound is not muted (`N`).
- **The window fails to open**: `wgpu` needs a working GPU / driver; check the
  console output for the backend error.

## Development

- Run the test suite without any game data: `cargo test --workspace`. Real-data
  integration tests self-skip unless `OPENJILL_DATA_DIR` (or the default data
  directory) is present.
- Per-format extraction and inspection run as `openjill <format> <action>`
  subcommands (DMA / SHA / VCL / CFG / JN), built into the same binary under its
  editor features (the default `editor-ui` enables the GUI viewers and implies
  the CLI-only `editor`); the [Taskfile](Taskfile.dist.yaml) wraps common
  workflows (`task --list`).
- Porting design notes and the original-format reference are in
  [`docs/port/`](docs/port/).

## License

Mozilla Public License 2.0 (`MPL-2.0`); see [LICENSE](LICENSE).
