# Original Data Format Reference

Byte-level reference for Jill of the Jungle data formats, distilled from the
ModdingWiki. Use this as the porter's source of truth for parser and runtime
behavior; it complements the per-phase subplans (`02-original-data-parsers.md`,
`04-render-input.md`, `06-episode-1-gameplay.md`) and the high-level notes in
`PORT.md`.

The original wiki pages were reverse-engineered by Malvineous, Ilovemyq3map2,
and SaxxonPike on the ModdingWiki and the authors request a backlink, so each
section below cites the upstream URL.

## Sources

- [Jill of the Jungle][wiki-jill] — engine overview, palette/EXE offsets,
  modding capability matrix, file extension table.
- [Jill of the Jungle Map Format][wiki-jn] — `*.JN[123]` background grid,
  object records, savegame block, string stack.
- [SHA Format][wiki-sha] — `*.SHA` archive header, tileset records, tile
  encodings, palette-override tile.
- [MAC Format (Jill of the Jungle)][wiki-mac] — `*.MAC` demo macro stream.
- [CFG Format (Jill of the Jungle)][wiki-cfg] — `*.CFG` high scores, save
  names, joystick/display/audio config.
- [VCL Format][wiki-vcl] — `*.VCL` sound (8-bit signed PCM) and text tables.
- [CMF Format][wiki-cmf] — `*.DDT` Adlib/OPL music (Creative Music Format).
- [Crunched Screen Image][wiki-csi] — TheDraw byte-code stream used for
  hardware-detect, ordering, and ending screens inside the loader EXE.
- [Using the official Jill of the Jungle level editor][wiki-editor] —
  in-game editor key bindings and the canonical object/iType name list
  extracted from the episode 1 EXE.
- [Category:Jill of the Jungle][wiki-cat] — index of every Jill page on
  the wiki (B800 Text, CGA Palette, Cheats, palette image, etc.).

[wiki-jill]: https://moddingwiki.shikadi.net/wiki/Jill_of_the_Jungle
[wiki-jn]: https://moddingwiki.shikadi.net/wiki/Jill_of_the_Jungle_Map_Format
[wiki-sha]: https://moddingwiki.shikadi.net/wiki/SHA_Format
[wiki-mac]: https://moddingwiki.shikadi.net/wiki/MAC_Format_%28Jill_of_the_Jungle%29
[wiki-cfg]: https://moddingwiki.shikadi.net/wiki/CFG_Format_%28Jill_of_the_Jungle%29
[wiki-vcl]: https://moddingwiki.shikadi.net/wiki/VCL_Format
[wiki-cmf]: https://moddingwiki.shikadi.net/wiki/CMF_Format
[wiki-csi]: https://moddingwiki.shikadi.net/wiki/Crunched_Screen_Image
[wiki-editor]: https://moddingwiki.shikadi.net/wiki/Using_the_official_Jill_of_the_Jungle_level_editor
[wiki-cat]: https://moddingwiki.shikadi.net/wiki/Category:Jill_of_the_Jungle
[wiki-dma]: https://moddingwiki.shikadi.net/wiki/DMA_Format

All multi-byte values are little-endian unless stated otherwise.

## Engine constants

From [Jill of the Jungle][wiki-jill]:

- Tile cell: `16x16` pixels.
- Viewport: `232x160` pixels (game area, excluding status bar).
- Display modes selectable at runtime: CGA, EGA, VGA. A single 8bpp source
  image is stored; the SHA color map projects it to lower-color hardware.
- Frame interval used by the OpenJill Java reference: `55ms`.
- Built-in editor: `Ctrl+E` from main menu; `Ctrl+P` plays the intro level as
  gameplay (level files are reused for screens, demos, and help).

### VGA palette inside the EXE

The VGA palette is stored inside the game EXE in classic VGA 6-bit-per-channel
RGB form and is identical across every episode and version. A SHA palette
override tile (see below) can replace it at runtime. Per-version offsets
(post-UNLZEXE for episodes 2 and 3 v1.0):

| Version    | Episode 1 | Episode 2 | Episode 3 |
|------------|-----------|-----------|-----------|
| 1.0        | `0x1ED84` | `0x1F0B4` | `0x1F1F4` |
| 1.2        | `0x1F884` | —         | —         |
| 1.2(a)     | `0x1FD54` | —         | `0x1F804` |
| 1.2(b)     | `0x1FC34` | `0x1F6E4` | `0x1F6E4` |
| 1.2(c)     | `0x1FC04` | `0x1F6B4` | `0x1F6B4` |
| 1.2(d)     | `0x24A64` | `0x26A64` | `0x26A64` |

The Rust port loads its palette through the SHA path (`Palette::from_sha`),
so the EXE table is here only for parity work and palette extraction tools.

## File extensions

| Extension          | Format                  | Purpose                              |
|--------------------|-------------------------|--------------------------------------|
| `*.cfg`            | CFG                     | Game configuration / high scores     |
| `*.ddt`            | CMF (id Software music) | Background music                     |
| `jill.dma`         | DMA                     | Tile metadata table                  |
| `*.jn[123]`        | Jill Map                | Levels, screens, intro, map, demos   |
| `*.mac`            | MAC                     | Demo macros (recorded input streams) |
| `*.sha`            | SHA                     | Tilesets, sprites, fonts, pictures   |
| `*.vcl`            | VCL                     | Sound and text assets                |
| `jn[123]save.[0-9]`| Jill Map                | Saved games (full level state)       |

## DMA — `JILL.DMA`

Sources: [DMA Format][wiki-dma] (entry layout), [Jill of the Jungle Map
Format][wiki-jn] §"Tile Mapping Table Entry" (full flag bit semantics).

Each entry is variable length:

| Field        | Type      | Notes                                          |
|--------------|-----------|------------------------------------------------|
| `iMapCode`   | `u16le`   | ID referenced from JN background grid.         |
| `iTile`      | `u8`      | Index within the chosen tileset.               |
| `iTileset`   | `u8`      | Mask with `0x3F`; upper 2 bits unknown.        |
| `iFlags`     | `u16le`   | Behavior bits — see "iFlags" below.            |
| `iLength`    | `u8`      | Length of name in bytes.                       |
| `cName`      | `u8[len]` | Tile name. **Not** null-terminated.            |

### iFlags

Flags are shared between tile entries and JN object records; many bits have
the same semantics in both contexts.

| Bit      | Name        | Meaning                                            |
|----------|-------------|----------------------------------------------------|
| `0x0001` | PLAYERTHRU  | Player walks through tile.                         |
| `0x0002` | STAIR       | Slope/stair surface.                               |
| `0x0004` | VINE        | Climbable.                                         |
| `0x0008` | MSGTOUCH    | Sends touch message on player overlap.             |
| `0x0010` | MSGDRAW     | Animated; redraw each frame.                       |
| `0x0020` | MSGUPDATE   | Receives per-frame update message.                 |
| `0x0040` | INSIDE      | "Inside" surface (cave/room).                      |
| `0x0080` | FRONT       | Drawn in front of the player.                      |
| `0x0200` | BACK/TINY   | Drawn behind player; small objects pass through.   |
| `0x0800` | KILLABLE    | Destroyable.                                       |
| `0x1000` | FIREBALL    | Fireball-only collision/destroy.                   |
| `0x2000` | WATER       | Water surface/volume.                              |
| `0x4000` | WEAPON      | Triggers weapon interaction.                       |

## SHA — `*.SHA`

Source: [SHA Format][wiki-sha].

### File header (768 bytes)

| Offset | Size      | Field                              |
|--------|-----------|------------------------------------|
| `0`    | 512 B     | `offsets[128]` (`u32le`)           |
| `512`  | 256 B     | `sizes[128]` (`u16le`)             |

Only the first 64 entries are populated in shipped files; the rest are zero.
First tileset starts at byte `768` (`128 * 4 + 128 * 2`). There is no magic
signature — validate by ensuring all offsets are within file bounds.

### Tileset header (10 bytes)

| Field           | Type    | Notes                                              |
|-----------------|---------|----------------------------------------------------|
| `numShapes`     | `u8`    | Tile count in this tileset.                        |
| `numRots`       | `u16le` | Generally `1`; "doesn't seem to have any use".     |
| `lenCGA`        | `u16le` | Decompressed memory size for CGA.                  |
| `lenEGA`        | `u16le` | Decompressed memory size for EGA.                  |
| `lenVGA`        | `u16le` | Decompressed memory size for VGA.                  |
| `numColourBits` | `u8`    | Source bit depth of the color map indices.         |
| `flags`         | `u16le` | `0x0001`=font, `0x0004`=level tileset.             |

### Optional color map

Present **unless** `flags & 0x0001` (font) is set or `numColourBits == 8`.
Size: `(1 << numColourBits) * 4` bytes. Each 4-byte entry holds:

| Byte | Meaning              |
|------|----------------------|
| `0`  | CGA palette index    |
| `1`  | EGA palette index    |
| `2`  | VGA palette index    |
| `3`  | Unused (zero)        |

### Tile entries

Each tile starts with a 3-byte header: `u8 width`, `u8 height`, `u8 type`.

| `type` | Encoding | Layout                                                |
|--------|----------|-------------------------------------------------------|
| `0`    | BYTE     | Raw 8bpp, length `width * height`. **Used by Jill.**  |
| `1`    | PLAIN    | Packed pixels at `numColourBits` bpp. Stride = `((width * bits) + 7) / 8`; total = `stride * height`. (Kiloblaster.) |
| `2`    | RLE      | "Apparently unused."                                  |

### Special palette tile

A tile with `numColourBits == 8` (no color map) and dimensions exactly
**64x12 = 768 bytes** is interpreted as a 6-bit VGA palette rather than image
data. Kiloblaster uses this for runtime palette overrides; only color indices
**15 through 240 inclusive** are written, others remain unchanged. Jill's
shipped data does not normally rely on this, but the parser should detect and
preserve it for modding compatibility.

### Constraints

- Maximum 128 tiles per file.
- Tile dimensions: `0..=255` in each axis.
- Single linear plane, palette-based transparency, no compression in practice.

## JN — `*.JN[123]`

Source: [Jill of the Jungle Map Format][wiki-jn].

File layout (sequential):

1. Background layer — fixed `16384` bytes.
2. Object layer — `u16le` count, then `count * 31` bytes of records.
3. Savegame block — fixed size, game-dependent (Jill: 70 B; Xargon: 97 B).
4. String stack — variable, referenced from object records.

### Background layer (offsets `0..=16383`)

- Grid: **128 wide × 64 tall** = 8192 cells.
- Each cell is a `u16le`. Total bytes: `8192 * 2 = 16384`.
- Indexing formula (column-major): `byte_offset = ((x * 64) + y) * 2`.
- **Lower 14 bits** (`code & 0x3FFF`) is the DMA `iMapCode` lookup.
- **Upper 2 bits** are runtime flags. Official maps set them, so writers
  should mirror the original byte rather than zeroing them.

### Object layer

First two bytes are a `u16le` count, then each object is **31 bytes**:

| Offset | Type    | Field          | Notes                                       |
|--------|---------|----------------|---------------------------------------------|
| `0`    | `u8`    | `iType`        | Object kind. `0` = PLAYER.                  |
| `1`    | `u16le` | `iX`           | Tile X.                                     |
| `3`    | `u16le` | `iY`           | Tile Y.                                     |
| `5`    | `u16le` | `iXD`          | X velocity (signed semantics).              |
| `7`    | `u16le` | `iYD`          | Y velocity.                                 |
| `9`    | `u16le` | `iWidth`       | Pixel width.                                |
| `11`   | `u16le` | `iHeight`      | Pixel height.                               |
| `13`   | `u16le` | `iState`       | State machine slot.                         |
| `15`   | `u16le` | `iSubState`    | Sub-state.                                  |
| `17`   | `u16le` | `iStateCount`  | Tick counter for current state.             |
| `19`   | `u16le` | `iCounter`     | Trigger/target link tag.                    |
| `21`   | `u16le` | `iFlags`       | Same flag bit map as DMA `iFlags`.          |
| `23`   | `u32le` | `lPointer`     | Nonzero ⇒ owns a string-stack entry.        |
| `27`   | `u16le` | `iInfo1`       | Per-type metadata.                          |
| `29`   | `u16le` | `iZapHold`     | "Zap hold" runtime field.                   |

**Critical porting rule:** the player object (`iType == 0`) **must be the
first record** in the object array, otherwise the original engine has a
scroll bug. Preserve order on save.

#### Selected `iType` values (Jill)

Numeric `iType` values are not exhaustively documented on the wiki, but the
[level editor page][wiki-editor] lists the full object name table extracted
from the episode 1 EXE. The mapping is positional in the EXE; the names below
appear in the order they were dumped:

```
PLAYER, APPLE, KNIFE, KILLME, BIGANT, MACROTRIG, DEMON, BUNNY, INCHWORM,
ZAPPER, BOBSLUG, CHECKPT, PAUL, WISEMAN, FATSO, FIREBALL, CLOUD, TEXT6,
TEXT8, FROG, TINY, DOOR, FALLDOOR, BRIDGER, SCORE, TOKEN, PHOENIX, FIRE,
SWITCH, TXTMSG, BOULDER, EXPL1, EXPL2, STALAG, SNAKE, SEAROCK, BOLL, MEGA,
KNIGHT, BEENEST, BEESWARM, CRAB, CROC, EPIC, SPINBLAD, SKULL, BUTTON,
JILLFISH, JILLSPIDER, JILLBIRD, JILLFROG, BUBBLE, JELLYFISH, BADFISH, ELEV,
FIREBULLET, FISHBULLET, VINECLIMB, FLAG, MAPDEMO, ROMAN
```

The wiki notes "not all objects are valid", so the Rust port should treat
unknown `iType` values as no-ops rather than aborting.

Per-iType behavior the wiki documents explicitly:

| iType | Name      | Notes                                                       |
|-------|-----------|-------------------------------------------------------------|
| `0`   | PLAYER    | Jill. Must be first object.                                 |
| `1`   | APPLE     | Pickup.                                                     |
| `2`   | KNIFE     | Pickup.                                                     |
| `3`   | KILLME    | **Sentinel for deleted objects.** The editor's "delete" reassigns the object's `iType` to `KILLME` rather than removing the record. Treat as inert/dead. |
| `12`  | CHECKPT   | Checkpoint. `iCounter` = level number; `iState == 1` resets level on death. |
| `14`  | KEY       | Pickup; `iCounter` matches DOOR `iCounter`.                 |
| `15`  | PAD       | Touch trigger; links via `iCounter`.                        |
| `20`  | TEXT6     | Reads string via `lPointer`; `iXD`=CGA color, `iYD`=background (`-1` transparent). |
| `21`  | TEXT8     | Same as TEXT6, larger font.                                 |
| `23`  | TINY      | Overhead-map Jill sprite.                                   |
| `24`  | DOOR      | `iYD` = key type; `iCounter` is its own tag.                |
| `26`  | BRIDGER   | Toggleable wall.                                            |
| `28`  | TOKEN     | Generic pickup; `iCounter` (a.k.a. "CNT" in the editor) selects the powerup variant `0..=9`. Subtype = inventory ID. |
| `32`  | SWITCH    | Trigger; links via `iCounter`.                              |
| `33`  | GEM       | Score pickup.                                               |
| `52`  | BUTTON    | Trigger; links via `iCounter`.                              |
| `61`  | ELEV      | Elevator/lift.                                              |

### Savegame block — Jill (70 bytes)

| Offset | Type    | Field             | Notes                                  |
|--------|---------|-------------------|----------------------------------------|
| `0`    | `i16le` | `level`           | Current level number.                  |
| `2`    | `i16le` | `health`          | `0..=8`.                               |
| `4`    | `i16le` | `inventoryLength` | Items currently held.                  |
| `6`    | `i16le` x 16 | `inventory[16]` | Inventory token IDs.                |
| `38`   | `u32le` | `score`           |                                        |
| `42`   | `u8` x 28 | `pad[28]`       | Reserved.                              |

#### Inventory token IDs (Jill)

| ID | Item                |
|----|---------------------|
| `0`| Jill morph (frog/etc)|
| `1`| Red key             |
| `2`| Knife               |
| `3`| Crystal             |
| `4`| Frog                |
| `5`| Firebird            |
| `6`| Coin bag            |
| `7`| Fish                |
| `8`| Blade               |
| `9`| High jump           |

(Xargon's 97-byte block is documented on the wiki; not needed for episode 1
parity but listed there for trilogy work.)

### String stack

Sequence of records, each: `u16le length`, then `length` bytes of payload,
then a **trailing null byte** (the length field does not include it). Read
`length + 1` bytes per record. Used by `iType` 12 (CHECKPT), 20 (TEXT6),
21 (TEXT8) via `lPointer`.

#### Checkpoint string prefixes

| Prefix      | Behavior                                                          |
|-------------|-------------------------------------------------------------------|
| `*filename` | Load and play song from the start.                                |
| `#filename` | Keep current song, or play if none / level 1–32.                  |
| `&filename` | Xargon: force `song_33.xr1` and treat remainder as demo macro filename. Jill: song unchanged. |
| `!`         | Load previous map. No filename follows.                           |
| (none)      | Filename is the next level to load.                               |

## MAC — `*.MAC`

Source: [MAC Format (Jill of the Jungle)][wiki-mac].

Demo playback stream. **No header.** Read events sequentially until EOF;
the first event applies at frame 0.

Each event is read in three sections.

### 1. Input flags (1 byte)

Bitmask indicating which inputs change this event. Inputs without their flag
set retain their previous state.

| Bit      | Input                            |
|----------|----------------------------------|
| `0x01`   | X-axis                           |
| `0x02`   | Y-axis                           |
| `0x04`   | Button 1 (typically Jump)        |
| `0x08`   | Button 2 (typically Shoot)       |
| `0x10`   | Keyboard key                     |

### 2. Stored input (0–5 bytes)

For each set flag, in flag-bit order, read 1 byte.

| Input    | Encoding                                                       |
|----------|----------------------------------------------------------------|
| X-axis   | `0xFF` = Left, `0x00` = Center, `0x01` = Right.                |
| Y-axis   | `0xFF` = Up,   `0x00` = Center, `0x01` = Down.                 |
| Button 1 | `0x00` released, `0x01` pressed.                               |
| Button 2 | `0x00` released, `0x01` pressed.                               |
| Key      | Raw key value; `0x00` = no key.                                |

### 3. Next-event timestamp (1–2 bytes)

- If byte `< 128`: that byte is the absolute frame number of the next event.
- If byte `>= 128`: clear bit 7 to get low 7 bits, then read another byte
  and shift it left by 7; sum to obtain the absolute frame number.

Frames are **absolute**, not deltas. Maximum frame number: `32767`.

### Determinism constraints

- Random seed is hardcoded to **`12345`** (not stored in the file). Demo
  playback only matches the original when the engine RNG is seeded with this
  value.
- Only one non-directional, non-fire keyboard key may be held at a time.

### Known shipped demos

| File          | Frames | Base map                    |
|---------------|--------|-----------------------------|
| `JN1DEMO.MAC` | 8647   | `INTRO.JN1` (uses `0/1/2.DEM`) |
| `JN2DEM1.MAC` | 2008   | `3.JN2`                     |
| `JN2DEM2.MAC` | 1771   | `9.JN2`                     |
| `JN2DEM3.MAC` | 2036   | `17.JN2`                    |
| `JN3DEM1.MAC` | 2231   | `1.JN3`                     |
| `JN3DEM2.MAC` | 1973   | `5.JN3`                     |
| `JN3DEM3.MAC` | 1957   | `12.JN3`                    |

## CFG — `JILL[1-3].CFG`

Source: [CFG Format (Jill of the Jungle)][wiki-cfg].

File size: 254 bytes.

| Offset    | Type           | Field                               |
|-----------|----------------|-------------------------------------|
| `0..=99`  | `char[10] x 10`| High-score names #1–10.             |
| `100..=119` | (undocumented) | **Preserve on round-trip.**       |
| `120..=159` | `i32le x 10` | High-score values #1–10.            |
| `160..=231` | `char[12] x 6` | Saved-game names #1–6.            |
| `232..`     | `CFG_STRUCT` | Common configuration block.         |

The wiki notes saved-game names display as 7 characters in-game even though
the field is 12 bytes wide; the Rust writer should keep the 12-byte field
intact and let the renderer truncate.

### Common configuration block (all `i16le`)

| Offset | Field                                                      |
|--------|------------------------------------------------------------|
| `0`    | Reset flag (1 = reset config; **one-shot**, engine clears).|
| `2`    | Joystick enabled (nonzero = on).                           |
| `4`    | Joystick X left.                                           |
| `6`    | Joystick X center.                                         |
| `8`    | Joystick X right.                                          |
| `10`   | Joystick Y up.                                             |
| `12`   | Joystick Y center.                                         |
| `14`   | Joystick Y down.                                           |
| `16`   | Display mode (Jill: `1`=CGA, `2`=EGA, `4`=VGA).            |
| `18`   | Music enabled (nonzero).                                   |
| `20`   | Digital sound enabled (nonzero).                           |

### Round-trip rules

- **Bytes 100–119 are undocumented but non-zero in shipped files.** Treat
  them as opaque blob; preserve verbatim on rewrite. Do not zero-fill.
- The reset flag at common-block offset 0 is a one-shot trigger. The engine
  clears it after honoring; the Rust port must do the same.
- No key-binding fields are present in this format. Input remapping, if any,
  lives elsewhere (or is absent in stock Jill).

## VCL — `*.VCL`

Source: [VCL Format][wiki-vcl].

Stores up to **50 sound entries** and **40 text entries**. Used by Jill of
the Jungle and Xargon. The Java OpenJill VCL parser only reads the text
side; the Rust port should follow the wiki spec for both.

The format's true name is "voclib" (Tim Sweeney, 1991); a Python reference
implementation lives in the `mrcrowbar` library and matches the offsets
below verbatim.

### Table layout (fixed offsets)

| Offset | Size  | Field                                       |
|--------|-------|---------------------------------------------|
| `0`    | 200 B | `soundOffsets[50]` (`u32le`)                |
| `200`  | 100 B | `soundLengths[50]` (`u16le`, bytes)         |
| `300`  | 100 B | `soundFrequencies[50]` (`u16le`, Hz)        |
| `400`  | 160 B | `textOffsets[40]` (`u32le`)                 |
| `560`  | 80 B  | `textLengths[40]` (`u16le`, bytes)          |
| `640`  | var   | Unknown auxiliary block (see below)         |
| ...    | var   | Sound + text payload heap                   |

Empty entries have `length == 0`.

### Unknown auxiliary block

Between byte 640 and the lowest data offset there is a small region of
format `u16le sequenceLength`, then `sequenceLength` bytes of payload.
`sequenceLength` may be 0. Preserve verbatim on any rewrite — its semantics
are not yet documented.

### Sound payload

- Encoding: 8-bit signed raw PCM (same byte semantics as Creative VOC wave
  data, no header).
- Sample rate: per-entry value from `soundFrequencies[]`. Jill ships sounds
  at ~6000 Hz; Xargon mostly uses 6024 Hz; other Sweeney engines have used
  6087.7 Hz and 6250 Hz.
- The Rust port should resample to whatever rate `rodio` is configured for
  rather than baking a single rate into the audio crate.

### Text payload

- Plain ASCII bytes, length given by `textLengths[]`.
- Each line is preceded by a **rendering-style byte** (a small integer index
  into a hardcoded table inside the EXE). Jill's style table differs from
  Xargon's; the Rust port must keep style indices opaque and look up colors
  from a table extracted from the EXE or transliterated by hand.

## CMF — `*.DDT` (background music)

Source: [CMF Format][wiki-cmf].

`*.DDT` files are Creative Music Format — Adlib/OPL2 instrument data plus
single-track MIDI-equivalent song data. Out of scope for episode 1 audio
priority but documented here so the audio phase has a starting point.

### File header

| Offset | Size | Field                | Notes                                  |
|--------|------|----------------------|----------------------------------------|
| `0x00` | 4    | `cSignature`         | ASCII `"CTMF"`. No null terminator.    |
| `0x04` | 1    | `iVersionMinor`      | Usually `0x01`.                        |
| `0x05` | 1    | `iVersionMajor`      | Usually `0x01`.                        |
| `0x06` | 2    | `iOffsetInstruments` | `u16le` offset to instrument block.    |
| `0x08` | 2    | `iOffsetMusic`       | `u16le` offset to song data.           |
| `0x0A` | 2    | `iTicksPerQuarter`   | Ticks per beat.                        |
| `0x0C` | 2    | `iTicksPerSecond`    | **Authoritative** for playback timing. |
| `0x0E` | 2    | `iOffsetTitle`       | 0 if absent.                           |
| `0x10` | 2    | `iOffsetComposer`    | 0 if absent.                           |
| `0x12` | 2    | `iOffsetRemarks`     | 0 if absent.                           |
| `0x14` | 16   | `iChannelInUse`      | Per-channel use flags (0 or 1).        |

Version-dependent tail at `0x24`:

- **v1.0**: `u8 iInstrumentCount`. Header ends at `0x25`.
- **v1.1**: `u16le iInstrumentCount`, then `u16le iTempo` (BPM, presumed).
  Header ends at `0x28`.

Title/composer/remarks are null-terminated ASCII at the offsets above; the
offsets are authoritative even when they point past EOF (validate before
reading — Highway Hunter ships with such offsets).

### Instrument block (16 bytes per entry)

| Offset | Field                          | OPL register base |
|--------|--------------------------------|-------------------|
| `+0`   | `iModChar` (Mult/KSR/EG/VIB/AM)| `0x20`            |
| `+1`   | `iCarChar`                     | `0x23`            |
| `+2`   | `iModScale` (KSL/output level) | `0x40`            |
| `+3`   | `iCarScale`                    | `0x43`            |
| `+4`   | `iModAttack/Decay`             | `0x60`            |
| `+5`   | `iCarAttack/Decay`             | `0x63`            |
| `+6`   | `iModSustain/Release`          | `0x80`            |
| `+7`   | `iCarSustain/Release`          | `0x83`            |
| `+8`   | `iModWaveSel`                  | `0xE0`            |
| `+9`   | `iCarWaveSel`                  | `0xE3`            |
| `+10`  | `iFeedback/Connection`         | `0xC0`            |
| `+11..+15` | Padding (5 bytes)          | —                 |

The 128 MIDI patch slots are pre-filled with 16 default instruments looped
8 times; the file's instruments overwrite slots from index 0.

### Song data

Raw MIDI track data without the `MTrk` header. Variable-length delta times
and running status apply.

Events used:

| Event   | Meaning                                                            |
|---------|--------------------------------------------------------------------|
| `0x80`  | Note off.                                                          |
| `0x90`  | Note on. Velocity is loaded near-directly into the OPL (logarithmic).|
| `0xC0`  | Program change. On rhythm-mode percussive channels, only half the OPL operator pair is loaded. |
| `0xB0 0x63` | AM+VIB depth (global). Bit 0 → VIB deep, bit 1 → AM deep. Default `3`. |
| `0xB0 0x66` | Marker byte (player-readable flag).                            |
| `0xB0 0x67` | Toggle melody/rhythm mode (global).                            |
| `0xB0 0x68` / `0xB0 0x69` | Transpose up/down in 1/128-semitone units (per channel, absolute). |
| `0xE0`  | Pitch bend. Ignored by Creative's `SBFMDRV`.                       |
| Sysex   | Effectively ignored; can break third-party players.                |

End-of-track marker: `FF 2F 00`.

### OPL channel mapping

OPL2 supplies 9 melodic channels, or 6 melodic + 5 percussion in rhythm
mode. Rhythm mode reserves MIDI channels 12–16 for bass drum, snare,
tom-tom, top cymbal, and hi-hat respectively. Percussion shares OPL channels
7/8/9, so e.g. snare and hi-hat share a frequency register — the last note
written wins. **Event ordering is load-bearing for percussion.**

### Per-file quirks the parser must tolerate

| File                | Quirk                                                             |
|---------------------|-------------------------------------------------------------------|
| Jill `dan.cmf`      | End-of-track is malformed as `FF 2F FE` instead of `FF 2F 00`. The trailing `FE` is a truncated/invalid VLQ byte. Treat any `FF 2F xx` as end-of-track. |
| Xargon `song_9.xr1` | Out-of-order note-off events. Track currently sounding note per channel and only honor a key-off whose pitch matches. |
| Xargon `song_32.xr1`| Zero-delta note-on/note-off pairs. SBFMDRV still produces audible output, so the port should not silently drop them. |

Related formats: SBI files share the same instrument record with an added
header; IBK files are banks of these records.

## Crunched Screen Image (loader EXE only)

Source: [Crunched Screen Image][wiki-csi].

The loader/episode-select EXE that ships with some Jill bundles is roughly
"90% raw display data", per [Jill of the Jungle][wiki-jill]. A single VGA
image is faded in by region.

| Data                | Loader-EXE offset | Size       |
|---------------------|-------------------|------------|
| Screen              | `0x04E7`          | 64,000 B   |
| Screen palette      | `0xFEE7`          | 768 B      |
| Order info (regions)| `0x12AF2`         | 23,630 B   |

### Stream format

No header. Read bytes until the cursor's Y-position falls off row 25.
Decompression starts with text attributes set to `0` (black on black, not
blinking) and the cursor at the top-left corner.

| Byte value | Meaning                                                       |
|------------|---------------------------------------------------------------|
| `0x00`–`0x0F` | Set foreground color to this value.                        |
| `0x10`–`0x17` | Set background color to lower 4 bits of this value.        |
| `0x18`        | Newline. Move cursor to start of the next row.             |
| `0x19`        | Read one byte `n`; emit `n + 1` space characters.          |
| `0x1A`        | Read two bytes `n` and `c`; emit `c` `n + 1` times.        |
| `0x1B`        | Toggle blinking text on/off.                               |
| `0x1C`–`0x1F` | Undefined; skip.                                           |
| `0x20`–`0xFF` | Output this character verbatim with the current attribute.|

Only relevant if porting the original episode-select shell or the
hardware-detect screen; the Rust CLI replaces those flows entirely. The
decompressed image is conceptually a B800 text-mode buffer (see [B800
Text][wiki-b800] on the wiki).

[wiki-b800]: https://moddingwiki.shikadi.net/wiki/B800_Text

## Modding capability matrix

For agents asking what the original engine actually exposes for modding
(useful when judging parity scope):

| Aspect          | Editable? |
|-----------------|-----------|
| Levels          | Yes       |
| Music           | Yes       |
| Story/cutscenes | Yes       |
| Tiles           | No        |
| Sprites         | No        |
| Sound           | No        |
| Text            | No        |
| UI/menus        | No        |
| Fullscreen      | N/A       |
| Demos           | Unknown   |

## Cross-references

- High-level porting plan and crate breakdown: [`PORT.md`](../../PORT.md).
- Phase 2 parser implementation notes:
  [`02-original-data-parsers.md`](02-original-data-parsers.md).
- Render/input pipeline: [`04-render-input.md`](04-render-input.md).
- Episode 1 gameplay subplan: [`06-episode-1-gameplay.md`](06-episode-1-gameplay.md).
