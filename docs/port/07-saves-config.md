# Epic 7 Subplan: Saves, High Scores, and Config

Implements local save slots, high-score persistence, and config (setup) handling
compatible with episode-1 `JILL1.CFG` expectations. Tracks GitHub issue #7.

The guiding rule: **the shipped/original data directory is read-only**; all
mutations (CFG updates, save games, high scores) go to a separate user-writable
runtime directory, and missing or read-only original data must never crash.

## Inspected OpenJill modules and Rust crates

Java reference:
- `cfg-file-api/.../CfgFile.java`, `HighScoreItem.java`, `SaveGameItem.java` — CFG API + row fields.
- `cfg-file/.../CfgFileImpl.java` — CFG parse **and write** logic (`load`, `addNewHighScore`, `addNewSaveGame`, `writeHighScoreName`, `writeHighScore`, `writeSaveName`, setup byte). Defines the on-disk byte order.
- `cfg-file/.../HighScoreItemImpl.java`, `SaveGameItemImpl.java` — field impls.
- `openjill-core/.../gui/menu/HighScoreMenu.java`, `LoadGameMenu.java`, `SaveGameMenu.java` — display/update flow (name entry, slot select, sorted insert).
- `openjill-core/.../level/cfg/JillLevelConfiguration.java` — per-episode `cfgSavePrefixe` (the `JN1`/`JILL1` prefix) feeding save-file names.

Rust crates:
- `openjill-data/src/cfg.rs` — **read-only** `CfgFile` parser (high scores, save slots, setup/joystick). No writer yet. Layout (in parse order): `HIGH_SCORE_COUNT` fixed-width space-filled names → `HIGH_SCORE_HOLE_LEN` hole bytes → `HIGH_SCORE_COUNT` i32-LE scores → `SAVE_SLOT_COUNT` fixed-width save names → `setup_flag` i16 → `joystick_flag` i16 → 6× joystick-calibration i16 → `display_mode` i16 → `music_flag` i16 → `sound_flag` i16.
- `openjill-data/src/lib.rs` — `DataDirectory`: case-insensitive **read** resolution + `open_reader`; no write surface (original data stays read-only).
- `openjill-game/src/orchestrator.rs` — already serialises world state in memory: `ScreenHandler::map_jn_bytes()` (MapScreen) and `level_jn_bytes()` (LevelScreen). These are the bytes a save slot persists.
- `openjill-export/src/cfg.rs` — debug exporters (read-only); not the runtime writer.

## Key decisions (decision-complete)

### 1. Writable runtime directory policy
- Resolve a per-user state directory once at startup, in priority order:
  1. `OPENJILL_STATE_DIR` env var (explicit override; used by tests).
  2. Platform data dir via the `dirs` crate: `dirs::data_dir()/openjill/` (Linux `~/.local/share/openjill`, Windows `%APPDATA%\openjill`, macOS `~/Library/Application Support/openjill`).
  3. Fallback: `std::env::temp_dir()/openjill/` if neither resolves.
- Add `dirs = "5"` (or current) to `[workspace.dependencies]`; only `openjill-game` depends on it.
- Per-episode subdir: `{state_dir}/{episode}/` (e.g. `JILL1/`) so episodes don't collide.
- The original `DataDirectory` is **never** written. New type `RuntimeDir` (in `openjill-game`) owns the writable path, creating it lazily (`create_dir_all`) and degrading to in-memory-only with a logged warning if creation fails (no panic).

### 2. CFG round-trip (extend `openjill-data::cfg`)
- Add `CfgFile::to_bytes(&self) -> Vec<u8>` mirroring `CfgFileImpl` write order exactly (names space-filled to slot width, hole, i32-LE scores, save names, setup/joystick/display/music/sound i16s).
- Preserve byte-faithfulness: store the raw `high_score_hole` bytes (currently skipped) and any trailing bytes after `sound_flag` on a new `raw_tail`/`hole` field, and re-emit them verbatim, so a written file is byte-identical to the original and the DOS game can still read it.
- Add mutators matching Java: `add_high_score(&mut self, name, score)` (insert keeping the top `HIGH_SCORE_COUNT` sorted desc, mirroring `addNewHighScore`) and `set_save_slot_name(&mut self, index, name)`.
- Round-trip test: parse original/synthetic bytes → `to_bytes` → byte-equal.

### 3. Save / load game
- A save slot persists the **world snapshot** as the original does: write the current map JN bytes to `{prefix}SAVEM.{index}` and the current level/game JN bytes to `{prefix}SAVE.{index}` inside the runtime dir, then `set_save_slot_name(index, name)` and persist the CFG.
- `prefix` comes from the episode descriptor's `cfgSavePrefixe` (e.g. `JILL1`), matching `CfgSaveSlot::save_game_file` (`{prefix}SAVE.{index}`) / `save_map_file` (`{prefix}SAVEM.{index}`).
- Restore reads those two files back and feeds the bytes to the orchestrator's existing level/map reconstruction path (reuse `map_jn_bytes`/`level_jn_bytes` round-trip already used for level restart).
- Save files are raw JN bytes (same container the DOS game writes) → cross-compatible.
- New `SaveStore` (in `openjill-game`) owns `RuntimeDir` + the loaded `CfgFile` and exposes `read_slots()`, `save(slot, name, map_bytes, level_bytes)`, `load(slot) -> (map_bytes, level_bytes)`, `record_high_score(name, score)`, all routing writes through `RuntimeDir` and persisting the CFG atomically (temp file + rename).

### 4. High scores
- On startup, `SaveStore` loads the **writable** CFG; if absent, seed it (see §5). High-score reads come from there.
- On game over with a qualifying score, the high-score flow (HighScoreMenu) prompts for a name, calls `record_high_score`, and persists. (Menu wiring is a child issue; the store API lands first.)

### 5. Seeding & read-only / missing original handling
- First run: if no writable CFG exists, seed it by copying the original `JILL1.CFG` bytes (read via `DataDirectory`) when present; otherwise build a default `CfgFile` (empty high-score names, zero scores, empty save names, setup defaults from a documented constant block).
- All original-data reads are guarded: missing original CFG → defaults; read-only original → only ever read; runtime-dir write failure → in-memory CFG + warning, app continues.
- No proprietary data committed: defaults are synthesised in code; tests use synthetic bytes and a tempdir runtime location.

### 6. Menus / UX surface
- Save / Load / High-score menus reached from the start menu and the in-level control panel keys already present (`S` = save, `R` = restore; high scores from the start menu). This is the largest UI piece and is split into its own child issue; the `SaveStore` API and CFG writer are prerequisites and land first so the menus are thin.

## Public interfaces, crate additions, and data types

`openjill-data` (`cfg.rs`):
- `CfgFile::to_bytes(&self) -> Vec<u8>`
- `CfgFile::add_high_score(&mut self, name: &str, score: i32)`
- `CfgFile::set_save_slot_name(&mut self, index: usize, name: &str)`
- internal: preserved `hole`/`raw_tail` bytes for faithful round-trip.

`openjill-game` (new `saves` module):
- `RuntimeDir` — resolves/creates the writable per-episode dir; `path()`, `read(file)`, `write_atomic(file, bytes)`.
- `SaveStore` — owns `RuntimeDir` + `CfgFile`; `load_or_seed(original: &DataDirectory, episode)`, `slots()`, `high_scores()`, `save(slot, name, map_bytes, level_bytes)`, `load(slot)`, `record_high_score(name, score)`, `persist_cfg()`.
- Orchestrator gains a `SaveStore` and wires the control-panel `S`/`R` and start-menu high-score paths to it.

Workspace deps: add `dirs`.

## Tests and acceptance checks
- CFG `to_bytes` round-trips a synthetic file byte-for-byte; `add_high_score` keeps a sorted top-N; `set_save_slot_name` updates the right slot.
- `SaveStore` against a `tempfile::TempDir`: save then load returns the same map/level bytes across a fresh store instance (simulates restart); high-score persists.
- Read-only / missing original: store seeds from defaults without the original CFG and never writes to the read-only dir; runtime-dir creation failure degrades gracefully.
- No proprietary save data in the repo; all fixtures synthetic.
- Acceptance (issue #7): existing high scores + save names readable; save/load across restarts; high-score updates persist; missing/read-only original does not crash.

## Child issues (suggested split)
1. `openjill-data`: CFG writer + mutators + round-trip tests (no runtime deps).
2. `openjill-game`: `RuntimeDir` + writable-dir policy (`dirs`, env override, atomic writes).
3. `openjill-game`: `SaveStore` (seed/load/persist CFG; save/restore JN snapshots) + tempdir tests.
4. `openjill-game`: wire save/restore to the control panel `S`/`R` and high-score recording on game over.
5. UI: Save / Load / High-score menus (start menu + in-level), name entry.

## Risks and handoff notes
- **Byte fidelity / hole bytes**: the parser currently discards the high-score hole; the writer must preserve it (and any trailing bytes) or the round-trip won't be byte-identical and DOS-readable. Capture them at parse time.
- **Save snapshot scope**: confirm the original `SAVE`/`SAVEM` contents equal the in-memory JN map/level bytes the orchestrator already serialises; if the DOS save adds a header, match it for cross-compat (otherwise document Rust-only save format).
- **Atomic persistence**: write CFG/saves to a temp file + rename to avoid corruption on crash.
- **Episode prefix**: source `cfgSavePrefixe` from the episode descriptor, not hard-coded, so multi-episode support (epic later) is unaffected.
