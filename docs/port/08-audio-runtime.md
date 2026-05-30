# Epic 8 Subplan: rodio Audio Runtime

Decision-complete subplan for issue #8. Goal: a sound-event API decoupled from
`rodio`, a `rodio` backend that plays the **original** Jill SFX, and the NOISE
mute wired through. Background music (`*.DDT`, CMF/Adlib OPL2) is **out of
scope** for this epic (documented as next-steps).

The guiding decision (chosen by the maintainer): **decode and play the original
VCL sounds**, not synthesized placeholders. The good news from investigation is
that this is tractable - the sounds are plain 8-bit signed PCM, not OPL/PC-speaker
synthesis.

## VCL sound-data investigation (done)

`*.VCL` ("voclib", Tim Sweeney 1991) stores up to 50 sound entries and 40 text
entries. The Rust parser currently **skips** the 400-byte sound region and reads
only the text side; the Java reference port does the same and has **no audio
playback code at all** - so there is no audio implementation to port, only the
on-disk format (documented in `docs/port/00-format-reference.md`).

Fixed table layout (little-endian):

| Offset | Size  | Field                         |
|--------|-------|-------------------------------|
| `0`    | 200 B | `soundOffsets[50]` (`u32`)    |
| `200`  | 100 B | `soundLengths[50]` (`u16`, bytes) |
| `300`  | 100 B | `soundFrequencies[50]` (`u16`, Hz) |
| `400`  | ...   | text tables (already parsed)  |

Empty entries have `length == 0`. Sound payload at `soundOffsets[i]` is
`soundLengths[i]` bytes of **8-bit signed raw PCM** (VOC wave semantics, no
header) at the per-entry sample rate.

Verified against the shipped `JILL1.VCL` (94954 bytes): **23 non-empty sounds**,
all `frequency = 6000` Hz, at sparse indices
`{1,2,3,4,5,6,8,10,11,12,15,16,18,19,23,24,25,28,33,35,39,41,48}`. Offsets are
absolute into the file and run sequentially within bounds.

Playback is therefore trivial: slice the PCM bytes, map `i8 -> f32`
(`s as f32 / 128.0`), hand `rodio` a single-channel buffer at the entry's sample
rate and let `rodio`/`cpal` resample to the output device rate.

The remaining unknown is **not** the format but the **index -> game-event
mapping**: which of the 23 sounds is jump, fire, pickup, etc. Neither port
records this. Ear calibration (extract all 23, listen, assign) pinned the four
**player** cues but could not identify the rest; recovering the remainder needs
`JILL.EXE` reverse-engineering. See "Sound index -> event mapping (open
follow-up)" below for the confirmed table and the RE recipe.

## Inspected modules and crates

- `crates/openjill-data/src/vcl.rs` - `VclFile`; today skips `SOUND_ENTRY_SKIP =
  400`. Extend to parse the sound table + expose decoded PCM. Keeps the text side.
- `crates/openjill-core/src/screen.rs` - `enum SoundEvent {}` (empty) and
  `TickResult::sound_events: Vec<SoundEvent>` already plumbed through every screen.
- `crates/openjill-core/src/runtime.rs` - `noise_enabled: bool` (defaults true),
  toggled by the `N` key (`InputCommand::ToggleNoise`); currently inert.
- `crates/openjill-audio/src/lib.rs` - one-line `AudioBackend` stub, no `rodio`.
- `crates/openjill-game/src/lib.rs` - `GameApp` (winit `ApplicationHandler`);
  already depends on `openjill-audio`. Owns the run loop and the asset cache.
- `crates/openjill-game/src/orchestrator.rs` - `tick()` returns only
  `Vec<RenderCommand>` and **discards** `TickResult::sound_events` today.

## Key decisions (decision-complete)

### 1. Decoupling boundary

`SoundEvent` (in `openjill-core`) is the rodio-free boundary. Gameplay and the
orchestrator only **produce** `SoundEvent`s; they never link `rodio`. `rodio`
lives solely in `openjill-audio`, consumed only by `GameApp`. The
`SoundEvent -> VCL index` mapping lives in `openjill-audio` (it is an
audio-backend concern, and keeps the core semantic).

### 2. VCL sound parser (`openjill-data`)

Parse the 50-entry table and retain the file bytes so payloads can be sliced.
Public surface:

```rust
pub struct VclSound { pub frequency: u16, pub pcm: Vec<i8> }
impl VclFile {
    /// Decoded sound at `index` (0..50), or None when the slot is empty.
    pub fn sound(&self, index: usize) -> Option<&VclSound>;
    pub fn sounds(&self) -> &[Option<VclSound>]; // length 50
}
```

Offsets are absolute; bounds-check each (`offset + length <= file_len`). Keeping
`openjill-data` **log-free** (see the "Parser error model" in
`docs/port/02-original-data-parsers.md`): a structurally invalid VCL still fails
`from_bytes` with a typed error, but an individual sound slot that is empty
(`length == 0`) **or** out of range / overlapping the table degrades to `None`
silently - the caller (the audio backend) decides whether to log a missing
sound, not the parser. The existing text parsing and `from_bytes` /
`text_entries` API are unchanged. VCL is read-only (no writer needed), so the
unknown auxiliary block at offset 640 is ignored, not preserved.

### 3. `SoundEvent` API (`openjill-core`)

Replace the empty enum with semantic, `#[non_exhaustive]` variants for the
episode-1 events that actually have emit points today:

```rust
pub enum SoundEvent {
    PlayerJump, PlayerFire, PlayerHurt, PlayerDie,
    ItemPickup, ExtraLife, EnemyHit,
    DoorOpen, SwitchToggle, LevelComplete,
    MenuMove, MenuSelect,
}
```

The final list is trimmed/extended in the mapping child issue to match the 23
available sounds; unmapped events are silently dropped by the backend.

### 4. Emitting events from gameplay (`openjill-game`)

Entities and screens push `SoundEvent`s into `TickResult::sound_events`.
Mechanism: a per-tick sound sink on the entity `MessageDispatcher` (parallel to
the existing message channel) that the `LevelScreen` drains into its
`TickResult`. Emit points (rising-edge / one-shot, mirroring the existing
message dispatches):

- `player.rs`: jump transition (`PlayerJump`), fire/throw (`PlayerFire`).
- `dispatch_player_touches` health-- (`PlayerHurt`), death (`PlayerDie`).
- pickup entities `on_touch` (`ItemPickup`; `ExtraLife` for 1-ups).
- projectile/enemy `on_kill` (`EnemyHit`).
- lock/door + switch entities (`DoorOpen`, `SwitchToggle`).
- level-exit / `pending` fire (`LevelComplete`).
- `start_menu.rs` nav/confirm (`MenuMove`, `MenuSelect`).

### 5. `rodio` backend (`openjill-audio`)

Add `rodio` (pin a known-good version; `cpal` ALSA/Pulse on Linux). Surface:

```rust
pub struct AudioBackend { /* stream, handle, decoded sounds, map, muted */ }
impl AudioBackend {
    pub fn new(vcl: &VclFile) -> Self;   // never panics; no device -> no-op
    pub fn play(&mut self, event: SoundEvent);
    pub fn set_muted(&mut self, muted: bool);
}
```

`new` decodes every mapped VCL sound to `f32` once and builds
`rodio::buffer::SamplesBuffer` (1 channel, entry frequency). `play` looks up the
`SoundEvent -> index`, and if unmuted and mapped, plays the buffer on the output
(short SFX, fire-and-forget; overlapping plays allowed). If the output device or
stream fails to initialize (headless / CI), the backend stores `None` and every
call is a logged no-op. `Drop` releases the stream.

### 6. Wiring + NOISE mute (`openjill-game` `GameApp`)

`GameApp` builds `AudioBackend::new(&cache.vcl)` at startup. The orchestrator
must stop discarding sound events: collect `TickResult::sound_events` into a
`last_sound_events` buffer (like `last_commands`) exposed via an accessor. In
`about_to_wait`, after `orch.tick(...)`, drain `orch.take_sound_events()` and
call `backend.play(e)` for each, then `backend.set_muted(!state.noise_enabled)`
so the `N` key mutes/unmutes.

### 7. Unsupported audio

Safe and visible: no audio device, an empty/missing VCL slot, or an unmapped
event are all no-ops with a one-time `debug!`/`eprintln` log. Background music
(`*.DDT`) is explicitly out of scope; documented here as the next audio epic.

## Public interface summary

- `openjill-data`: `VclSound`, `VclFile::sound`, `VclFile::sounds`.
- `openjill-core`: populated `enum SoundEvent` (semantic, `#[non_exhaustive]`).
- `openjill-audio`: `AudioBackend::{new, play, set_muted}` + `Drop`.
- `openjill-game`: orchestrator `take_sound_events()`; `GameApp` owns the backend.

## Tests and acceptance checks

- `openjill-data`: parse `JILL1.VCL` -> 23 non-empty sounds, all `freq == 6000`,
  each payload slice length == declared length, offsets in-range; synthetic
  fixture round-trip; empty-slot handling.
- `openjill-core`: `SoundEvent` variants compile and are `Clone + Eq`.
- `openjill-game`: each emit point pushes the expected `SoundEvent` into
  `TickResult::sound_events` (player jump, fire, pickup, hurt, die, etc.);
  orchestrator surfaces them via `take_sound_events()`.
- `openjill-audio`: `i8 -> f32` decode length/scale; mute gates `play`; missing
  device yields a no-op backend (construct with a null/again-fallible path).
- Manual (level 1): jump, throw a knife, grab a pickup, take a hit, die - each
  triggers a distinct original sound; `N` toggles silence.

Acceptance (issue #8): runtime emits sound events without depending on `rodio`
(✓ via `SoundEvent`); backend initializes and shuts down cleanly (✓ stream +
`Drop`, no-device no-op); supported sounds play through `rodio` (✓ decoded VCL
PCM); unsupported data documented with next-steps (✓ music + unmapped indices).

## Child issues (suggested split)

1. **openjill-data: VCL sound-table parser** - `VclSound`, `sound`/`sounds`,
   bounds-checks, tests against `JILL1.VCL`.
2. **openjill-core + openjill-game: `SoundEvent` API + gameplay emission** -
   populate the enum, push events at every emit point, drain into `TickResult`,
   surface via the orchestrator.
3. **openjill-audio: rodio backend** - dep, decode, `play`/`set_muted`,
   graceful no-device, `Drop`.
4. **openjill-game: wire backend into `GameApp` + NOISE mute** - own the
   backend, drain orchestrator sound events, mute on `N`.
5. **Index -> event mapping calibration** - extract + listen to the 23 sounds,
   assign each `SoundEvent`, document the table + confidence, log the unmapped.

## Sound index -> event mapping (open follow-up)

`openjill-audio::vcl_index` maps each `SoundEvent` to a VCL sound slot. The
**player** cues are confirmed by ear against `JILL1.VCL`:

| `SoundEvent` | VCL slot | Status    |
|--------------|----------|-----------|
| `PlayerJump` | 1        | confirmed |
| `PlayerFire` | 2        | confirmed |
| `PlayerHurt` | 3        | confirmed |
| `PlayerDie`  | 4        | confirmed |

The remaining **19** non-empty slots are unassigned (sample lengths in frames,
all 6000 Hz): `05`=4226 `06`=8160 `08`=1918 `10`=3013 `11`=3198 `12`=6253
`15`=638 `16`=2558 `18`=1918 `19`=3838 `23`=2558 `24`=2558 `25`=3198 `28`=1918
`33`=6398 `35`=2170 `39`=3838 `41`=7551 `48`=11518 (plus `01`-`04`, taken).

Future cues (`ItemPickup`, `ExtraLife`, `EnemyHit`, `DoorOpen`, `SwitchToggle`,
`LevelComplete`, `MenuMove`, `MenuSelect`) need a slot each. **The mapping could
not be determined by ear**, and is **not** present in the ported data: the
OpenJill config (`object_conf.json`) and Java reference carry no sound fields,
and the ModdingWiki documents only the VCL *format*. It exists only in the
original `JILL.EXE` (the code that calls play-sound with an index per event).

### How to recover it (one-time RE task)

The shipped `JILL.EXE` is MZ / Turbo C 2.0 with no packer signature; `dosbox-x`
(with its debugger) is available. **Never commit `JILL.EXE`, any disassembly, or
extracted audio - keep all RE artifacts in gitignored dirs.**

**What the symbol table already reveals (static recon, 2026-05-30):** the EXE
retains its Turbo C publics as a packed NUL-separated name blob near the end of
the load module (names only, no inline addresses). It names both the sound API
and the event handlers, which pins the breakpoint target and the call sites:

- **Sound API:** `_soundadd1` plays a sound by index, alongside `_soundadd`,
  `_soundadd2`, `_sampadd1`, `_sampadd`, `_makesound`, `_soundstop`,
  `_soundcount`, `_soundlen`, `_soundptr`. SFX run through the **WORX TOOLKIT
  2.01** Sound Blaster library (Mystic Software, 1993); `jill1.vcl` is the
  sample bank.
- **Event handlers that should call it:** `_msg_fire` / `_msg_firebullet` /
  `_msg_fireball`, `_msg_door` / `_msg_falldoor` / `_first_opendoor`,
  `_msg_switch` / `_first_switch`, `_msg_key` / `_first_key`, `_msg_score` /
  `_addscore` / `_kindscore`, `_hitplayer`, `_playerkill`, `_levelwin` /
  `_donelevelmsg`, `_domenu` / `_menu1` / `_menu2`, `_dolevelsong`.

Two routes (both now anchored on `_soundadd1`):

1. **Dynamic (most reliable):** in the `dosbox-x` debugger, `bp _soundadd1`. The
   first argument is the sound index; trigger each event in-game, read the index
   off the stack, and attribute it to whichever `_msg_*` handler sits on the
   call stack. This needs a human at the controls to drive each event.
2. **Static:** with a Borland symbol-table parser (`tdump`) or a 16-bit
   disassembler (Ghidra / IDA - **not installed here**, so this route is
   currently blocked), resolve `_soundadd1`'s address, list its callers, read
   the constant index pushed at each call site, and cross-reference the caller
   against the `_msg_*` handler names above.

Audition the candidates with `task data:vcl:sounds` (writes
`extract/sounds/sound-NN.wav`). Once known, update `vcl_index` (and add the
`SoundEvent` variants + emit points) - the mapping is a single constant table,
cheap to extend.

> `AUDIO.EPC` was ruled out: it is waveform data (built by `SMAKE.EXE`), not an
> event map.

## Risks and handoff notes

- **Index -> event mapping needs EXE RE**, not just listening (see the section
  above). The 4 player cues are confirmed; the rest await the one-time RE task.
  Static recon has narrowed it to `bp _soundadd1` (first arg = sound index) - a
  short dynamic `dosbox-x` session, since no static disassembler is installed.
  The mapping is one constant table in `openjill-audio`, cheap to correct.
- **Headless / CI device init must not panic.** `AudioBackend::new` degrades to
  a logged no-op. Gameplay tests never construct it.
- **`rodio`/`cpal` platform variance** (Linux ALSA vs Pulse). Pin a known-good
  `rodio`; keep the dep isolated to `openjill-audio`.
- **Resampling**: 8-bit PCM at 6000 Hz, upsampled by `rodio` to the device rate.
  Acceptable for short SFX; revisit only if artifacts appear.
- **Background music** (`*.DDT`, CMF/OPL2) is out of scope - a future epic.
