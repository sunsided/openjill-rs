# Epic 7 Prerequisite: JN State-Model Re-alignment

Prerequisite refactor for byte-faithful save/restore (issue #7). The save
*format* is already byte-faithful (`JnFile::to_bytes`, PR #174), but the running
game cannot produce or restore a faithful snapshot because the port's runtime
models have diverged from the JN integer model in three independent ways. This
subplan re-aligns them so a `JnObject` round-trips losslessly through every
entity and inventory item.

The guiding rule: **a parsed `JnObject` (and the inventory codes it implies)
must survive a full trip through the runtime model and back unchanged.** That
round-trip is the verifiable property that stands in for the DOS save fixtures
we do not have.

## Why this is needed (the three divergences)

Found while wiring the live save snapshot on top of PR #173/#174:

1. **Entity state model.** Rust entities use bespoke representations
   (`PlayerStateKind` enum, `jump_counter` / `on_floor` bools, etc.) that do not
   map 1:1 to a `JnObject`'s integer `state` / `sub_state` / `state_count` /
   `counter` / `info1` / `zap_hold` / `flags` fields. The Java managers store
   their working state *in* those `ObjectItem` integer fields, so the canonical
   mapping is "whatever fields the Java manager reads and writes".
2. **Constructors discard state.** Every `*Entity::new(item)` reads only
   `x` / `y` / `width` / `height` and resets everything else to defaults, so
   even a perfect snapshot could not be restored mid-state.
3. **Inventory codes diverged.** Java `EnumInventoryObject` is
   `JILL=0, RED_KEY=1, KNIVE=2, GEM=3, FROG=4, FIREBIRD=5, BAG_OF_COIN=6,
   FISH=7, BLADE=8, HIGH_JUMP=9, INVINCIBILITY=10`. The Rust `InventoryObject`
   is a 7-variant subset (`Jill, Gem, Key, Knife, Blade, FireFlower, Firebird`)
   with no faithful index round-trip.

None is verifiable against external bytes (no DOS `*SAVE.0` fixtures), so the
re-alignment is validated by the self-consistency round-trip below, not by
diffing distribution saves.

## Reference (Java)

- `open-jill-object-background/.../obj/*Manager.java` — each manager's
  `msgUpdate` / constructor shows which `ObjectItem` integer fields hold the
  live state (`getState`, `getSubState`, `getCounter`, `getInfo1`, `getZapHold`,
  speeds), and `ObjectItem.writeToFile` / the read path defines the byte order.
- `cfg-file`/`jn-file` `SaveData` write path (`AbstractChangeLevel`
  `writeBackgroundInFile` / `writeObjectInFile` / `writeSaveDataInFile`) — the
  canonical save layout (already mirrored by `JnFile::to_bytes`).
- `openjill-core-api/.../inventory/EnumInventoryObject.java` — the 11 inventory
  indices.

## The verifiable invariant

For every entity type and every inventory item:

```
parse JnObject -> Entity::new(&obj) -> entity.snapshot() == obj   (for the
fields the JN model defines)
from_index(item.index()) == item                                  (inventory)
```

A per-entity unit test asserts this fixed point. Fields the Java manager never
persists (e.g. transient render-only members) stay out of the `JnObject` and are
re-derived on `new`; everything the manager keeps in `ObjectItem` must round-trip
exactly. This is the contract that makes "exact parity" real *and* testable
without distribution saves.

## Approach and PR split

### 1. Inventory model (most bounded; lands first)

- Expand `InventoryObject` to mirror `EnumInventoryObject` (11 variants, JN
  indices 0..10).
- Add `InventoryObject::index() -> u16` and `from_index(u16) -> Option<Self>`.
- Update pickups, the status-bar inventory grid renderer, and every `match` on
  the enum to cover the new variants.
- Tests: `from_index(x.index()) == Some(x)` for all variants; unknown index ->
  `None`.

### 2. Entity state model (grouped sweeps)

Convention: every `ObjectEntity` reads its full live state from the `JnObject`
in `new` and writes it back in `snapshot`, mapping to the Java manager's
`ObjectItem` field usage. Rust enums (e.g. `PlayerStateKind`) gain an explicit
int encoding matching the Java state constants, with documented
`enum <-> i16` conversion (tagged `REVERSE-ENGINEERED:` when the encoding comes
from the EXE / Java constants rather than a data file).

Sweep order (each its own PR, each adds the round-trip test and keeps existing
gameplay tests green):

1. Player (`player.rs`) — the richest state; establishes the enum-encoding
   pattern.
2. Pickups / keys / collectibles (`apple`, `red_key`, `rock_key`, `point`,
   `bonus`, `gem`-likes, `text_tile`, `huge_letter_tile`).
3. Enemies (`frog`, `bees`, `crab`, `snake`, `ghost`, `skull`, `gator`,
   `giant_ant`, `firebird_enemy`, `eyes`, `spark`, `hive`) — most share the
   `enemy_shared` movement helpers.
4. Hazards / world objects (`lock_door`, `toggle_wall`, `switch`,
   `collapsing_ceiling`, `falling_spike`, `rolling_rock`, `flame`, `hit_fire`,
   `lift`, `bubbles`, `kill_water`-adjacent, `checkpoint`, `touch_trigger`).
5. Projectiles / spawned (`knife`, `bullet`, `blade`, `firebird_weapon`,
   `firebird_player`, `scatter_particle`) — these are created at runtime, so
   they need `JnObject::spawned(type, x, y, w, h)` plus full-field setters; some
   may legitimately be transient (`snapshot -> None`) if the Java reference does
   not persist them, documented per type.
6. `stub` — carries its origin `JnObject` verbatim so unknown types still
   round-trip.

### 3. Save/restore wiring (resumes epic #7 saves on top of the aligned model)

- `openjill-data`: `JnObject` field setters + `spawned()` ctor; `JnFile`
  mutators (`set_background_code`, `set_objects`, `set_save_data`).
- `openjill-core`: `ObjectEntity::snapshot(&self) -> Option<JnObject>` (default
  `None`); confirm `BackgroundGrid` exposes live `dma_map_code` per cell.
- `openjill-game`: `LevelScreen::snapshot_jn(&self, &RuntimeState) -> JnFile`
  (live background + `objects.filter_map(snapshot)` + save-data from
  `RuntimeState`); `ScreenHandler::snapshot_jn_bytes(&self, &RuntimeState)`;
  orchestrator `save_to_slot` uses the snapshot; `restore_from_slot`
  reconstructs the level + seeds `RuntimeState` from `save_data`.
- Menus (slot picker + name entry, high-score table) + control-panel S/R +
  high-score-on-game-over.

## Save-data <-> RuntimeState mapping

`writeSaveDataInFile` order is `level (u16), life (u16), inventory_count (u16),
inventory[count] (u16), padding to 16 slots, score (u32), 28-byte hole`:

- `level` <- current level number.
- `life` <- `RuntimeState::health`.
- `inventory` <- `RuntimeState::inventory` mapped via `InventoryObject::index()`.
- `score` <- `RuntimeState::score`.
- Padding and hole are zeroed on a fresh save (the Java `FileAbstractByte` is
  zero-initialized and `skipBytes` leaves zeros). `lives` is **not** in the save
  block (the original does not persist it); document the chosen behavior on
  restore (carry the current `lives`).

## Risks

- **Epic-6 regression.** Every entity is touched. Each PR must keep the existing
  gameplay unit tests green and honor the `PORT-FINDINGS.md` gameplay invariants
  (enemy gap-skip, hazard `on_kill`, level-entry restore snapshot, stair opt-out
  bit). Playtest after each merge.
- **Reverse-engineered enum encodings.** State int encodings come from the Java
  constants, not a data file -> tag `REVERSE-ENGINEERED:` and cite the Java
  source.
- **Unverifiable against distribution saves.** Mitigated by the self-consistency
  round-trip test; note in the save PR that byte-identity with DOS saves is
  asserted only structurally, not against captured originals.

## Deliverables checklist

- [ ] `InventoryObject` mirrors `EnumInventoryObject` with `index`/`from_index`.
- [ ] Every `ObjectEntity` round-trips `JnObject` (`new` reads, `snapshot`
      writes) with a per-entity fixed-point test.
- [ ] `JnObject`/`JnFile` save-build API + `ObjectEntity::snapshot`.
- [ ] `LevelScreen::snapshot_jn` + orchestrator `save_to_slot`/`restore_from_slot`.
- [ ] Save / load / high-score menus + control-panel S/R + high-score-on-game-over.
