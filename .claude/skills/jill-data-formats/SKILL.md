---
name: jill-data-formats
description: "Use when reading, parsing, writing, dumping, or debugging any Jill of the Jungle data file (DMA, SHA, JN, MAC, CFG, VCL, CMF/DDT, Crunched Screen Image) or when reasoning about its byte layout, flag bits, object iTypes, savegame structure, palette handling, or per-file quirks. Examples: \"why is this DMA entry parsing wrong?\", \"add a SHA tile dumper\", \"what's the layout of the JN object record?\", \"is the player object always first?\", \"decode dan.cmf\", \"parse the VCL sound table\", \"port the Crunched Screen Image renderer\"."
---

# Jill of the Jungle Data Formats

Single source of truth for byte-level layout of every Jill of the Jungle
data file: [`docs/port/00-format-reference.md`](../../../docs/port/00-format-reference.md).
That file cites the upstream ModdingWiki pages and is updated when new
porting questions surface. This skill is a router into it plus the
quirks/gotchas that bite porters most often.

## When to use

Trigger on any of:

- Touching `crates/openjill-data/` (or any module that reads/writes
  original Jill bytes).
- Adding or fixing a parser, dumper, or extractor.
- Designing types that mirror an on-disk record.
- Debugging why parsed data disagrees with the original game.
- Answering "what does this byte mean?" / "what is the layout of …?".
- Auditing endianness, offsets, masks, or string encoding for any of
  DMA, SHA, JN (`*.JN[123]`), MAC, CFG, VCL, CMF (`*.DDT`), or Crunched
  Screen Image streams.

## Workflow

```
1. Open docs/port/00-format-reference.md.
2. Jump to the section for the format in question (anchors below).
3. Re-read the per-file quirks subsection — most parser bugs live there.
4. Cross-check against existing Rust code in crates/openjill-data/<format>.rs
   (parser already exists for: DMA, VCL-text, CFG, SHA, JN).
5. For new behavior, prefer extending the existing parser to introducing
   a parallel one. Match the wiki's field names (iType, lPointer, etc.)
   so the code maps 1:1 onto the reference.
```

## Format index → reference anchor

| Format                | When you hit it                               | Reference section in 00-format-reference.md |
|-----------------------|-----------------------------------------------|---------------------------------------------|
| **DMA** (`JILL.DMA`)  | Tile metadata; iMapCode → tileset+flags       | `## DMA — JILL.DMA`                         |
| **SHA** (`*.SHA`)     | Tilesets, fonts, sprites, palette override    | `## SHA — *.SHA`                            |
| **JN** (`*.JN[123]`)  | Maps, screens, intro, savegames, string stack | `## JN — *.JN[123]`                         |
| **MAC** (`*.MAC`)     | Demo playback macros                          | `## MAC — *.MAC`                            |
| **CFG** (`JILL#.CFG`) | High scores, save names, joystick/display     | `## CFG — JILL[1-3].CFG`                    |
| **VCL** (`*.VCL`)     | 50 sounds + 40 texts                          | `## VCL — *.VCL`                            |
| **CMF** (`*.DDT`)     | Adlib/OPL background music                    | `## CMF — *.DDT (background music)`         |
| **Crunched Screen**   | Hardware-detect + ordering inside loader EXE  | `## Crunched Screen Image (loader EXE only)`|

## High-leverage quick-reference

These are the answers porters re-derive most often. **Always confirm
against the reference doc before coding** — this is a memory aid, not the
spec.

### Endianness

Everything multi-byte is little-endian. Period. Strings are not
null-terminated unless explicitly noted (DMA `cName` is **not**, CMF
title/composer/remarks **are**).

### DMA `iFlags` bits (shared with JN object `iFlags`)

```
0x0001 PLAYERTHRU   0x0008 MSGTOUCH   0x0080 FRONT       0x1000 FIREBALL
0x0002 STAIR        0x0010 MSGDRAW    0x0200 BACK/TINY   0x2000 WATER
0x0004 VINE         0x0020 MSGUPDATE  0x0800 KILLABLE    0x4000 WEAPON
                    0x0040 INSIDE
```

`iTileset` byte must be masked with `0x3F` — upper 2 bits are unknown
runtime flags.

### SHA layout in one paragraph

768-byte file header (128 × `u32le` offsets, then 128 × `u16le` sizes).
First tileset starts at byte 768. Each tileset: 10-byte header, optional
color-map (skip if `flags & 0x0001` font OR `numColourBits == 8`),
then tile records `(u8 width, u8 height, u8 type, payload)`. Jill uses
type `0` (raw 8bpp) exclusively. **Special palette tile**: type `0`,
8bpp, exactly 64×12 = 768 bytes ⇒ replaces VGA palette indices 15..240.

### JN file structure

```
[16384 B background grid] [u16le objCount] [objCount × 31 B object record]
[savegame block: Jill = 70 B, Xargon = 97 B] [string stack]
```

Background grid is 128×64 cells of `u16le`; **lower 14 bits** are the
DMA `iMapCode`, **upper 2 bits** are runtime flags (preserve verbatim).
Indexing: `((x * 64) + y) * 2`.

Object record fields in order (all `u16le` unless noted): `iType (u8)`,
`iX, iY, iXD, iYD, iWidth, iHeight, iState, iSubState, iStateCount,
iCounter, iFlags`, `lPointer (u32le)`, `iInfo1, iZapHold`. Total 31 B.

**Player (iType 0) MUST be first object** — original engine has a scroll
bug otherwise. Preserve order on save.

String-stack records: `u16le length`, `length` bytes, **trailing null
not in length** — read `length + 1`.

### Object iType reminders

- `iType 3 = KILLME` is the editor's deletion sentinel — treat as inert.
- `iType 28 = TOKEN` is the generic powerup; `iCounter` (0..=9) selects
  the inventory variant (Jill morph, red key, knife, crystal, frog,
  firebird, coin bag, fish, blade, high jump).
- `iType 24 = DOOR` matches `iCounter` against `iType 14 = KEY` /
  `iType 32 = SWITCH` / `iType 52 = BUTTON` triggers.
- `iType 20/21 = TEXT6/TEXT8` use `iXD` as foreground color, `iYD` as
  background (`-1` = transparent), and read text via `lPointer`.
- Full name table is at the bottom of the JN section in the reference.

### Savegame block — Jill (70 bytes, immediately after object array)

```
i16le level | i16le health(0..=8) | i16le invLen | i16le inv[16]
u32le score | u8[28] pad
```

### String-stack checkpoint prefixes (iType 12 CHECKPT)

```
*filename  → load song from start
#filename  → keep current song or play if none
&filename  → Xargon: force song_33.xr1 + treat as demo macro
            Jill: song unchanged
!          → load previous map (no filename)
no prefix  → next level filename
```

### MAC event loop

```
1. Read u8 inputFlags (bit 0=X, 1=Y, 2=Btn1, 3=Btn2, 4=Key)
2. For each set bit (LSB first), read 1 byte of input state.
   X/Y axis: 0xFF = neg, 0x00 = center, 0x01 = pos.
   Buttons: 0x00 / 0x01.
   Key:     raw key value, 0x00 = none.
3. Read next-event timestamp:
   - byte < 0x80  → absolute frame number
   - byte >= 0x80 → low 7 bits, then read another byte and shift << 7
```

Determinism: max 32767 frames; **RNG seed hardcoded to 12345**, not in
file.

### CFG layout (254 bytes total)

```
0..=99    char[10] x 10  high-score names
100..=119 (undocumented; PRESERVE on round-trip)
120..=159 i32le x 10     high-score values
160..=231 char[12] x 6   save names (display truncates to 7)
232..     CFG_STRUCT     i16le block (joystick/display/audio)
```

CFG_STRUCT offset 0 is a **one-shot reset flag** the engine clears after
honoring. Display mode: `1`=CGA, `2`=EGA, `4`=VGA.

### VCL fixed-offset tables (byte 0..=639)

```
0   sound_offsets[50]    (u32le)
200 sound_lengths[50]    (u16le, bytes)
300 sound_freqs[50]      (u16le, Hz; Jill ≈ 6000)
400 text_offsets[40]     (u32le)
560 text_lengths[40]     (u16le, bytes)
640 unknown aux block (u16le seqLen + seqLen bytes; preserve verbatim)
... payload heap (sounds = 8-bit signed PCM, text = ASCII with style prefix byte)
```

### CMF (`*.DDT`) header sketch

```
0x00 'CTMF'              (no NUL)
0x04 versionMinor, versionMajor (u8 each)
0x06 offsetInstruments   (u16le)
0x08 offsetMusic         (u16le)
0x0A ticksPerQuarter     (u16le)
0x0C ticksPerSecond      (u16le)   ← authoritative for playback
0x0E offsetTitle / 0x10 offsetComposer / 0x12 offsetRemarks (u16le)
0x14 channelInUse[16]    (u8 each)
0x24 v1.0: u8 instCount  | v1.1: u16le instCount, u16le tempo
```

Song data: raw MIDI track payload (no `MTrk` header). End-of-track:
`FF 2F 00`. **Jill `dan.cmf` ships malformed `FF 2F FE`** — treat any
`FF 2F xx` as end-of-track.

### Crunched Screen Image byte codes

```
0x00..0x0F  set foreground color
0x10..0x17  set background color (lower 4 bits)
0x18        newline
0x19 n      emit (n+1) spaces
0x1A n c    emit char c (n+1) times
0x1B        toggle blink
0x1C..0x1F  undefined; skip
0x20..0xFF  emit byte verbatim
```

No header. Stream ends when cursor Y exceeds row 25.

## Cross-cutting porting rules

- **Preserve unknown bytes verbatim** on round-trip writes. The
  background-grid upper 2 bits, the CFG `100..=119` region, and the VCL
  unknown-aux block all fall here.
- **Treat unknown `iType` as no-ops**, not errors — the wiki warns "not
  all objects are valid" and `KILLME` is normal.
- **Keep parser types decoupled from rendering.** `openjill-data` returns
  indexed pixels and the EXE-style palette; conversion to RGBA happens
  in `openjill-render`.
- **Mirror the wiki's field names** in Rust types (`iType`, `iCounter`,
  `lPointer`, `numColourBits`, …) so docs map 1:1 onto code. Use
  doc comments (per `AGENTS.md`) to spell out the meaning rather than
  renaming the fields.
- **Real-data integration tests** for any new parser, gated on
  `OPENJILL_DATA_DIR` (see `AGENTS.md` §"Integration tests").

## When the reference is wrong

If you find a parser disagreement with the wiki that the wiki doesn't
explain (e.g. another Jill-specific quirk like `dan.cmf`):

1. Update `docs/port/00-format-reference.md` with the new quirk under
   the relevant "Per-file quirks" / "Round-trip rules" subsection.
2. Cite the source (a specific shipped file, a Java OpenJill behavior,
   or a wiki diff).
3. Make the parser tolerate the malformed data; do not "correct" the
   bytes silently.

## See also

- [`docs/port/00-format-reference.md`](../../../docs/port/00-format-reference.md)
  — full byte-level spec.
- [`PORT.md`](../../../PORT.md) — porting plan and crate breakdown.
- [`AGENTS.md`](../../../AGENTS.md) — repo-wide agent rules
  (documentation, integration tests, data handling).
- [`docs/port/02-original-data-parsers.md`](../../../docs/port/02-original-data-parsers.md)
  — Rust parser implementation status and conventions.
