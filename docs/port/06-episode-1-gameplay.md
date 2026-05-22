# Epic 6 Subplan: Episode 1 Gameplay

## Inspected OpenJill modules and Rust crates

Java reference modules inspected:

- `openjill-core-api`: `ObjectEntity`, `BackgroundEntity`, `ObjectParam`,
  `BackgroundParam`, `KeyboardLayout`, `EnumMessageType`, `JillConst`.
  These define every contract that object and background managers implement.
- `open-jill-object-background` (objects): `PlayerManager`,
  `AbstractPlayerManager`, `AbstractPlayerInteractionManager`,
  `AbstractParameterObjectEntity`, `AbstractHitPlayerObjectEntity`,
  `AbstractFireHitPlayerObject`, `PalyerActionPerState`, `PlayerState`,
  `PlayerJumpingConst`, `PlayerStandConst`, `PlayerClimbConst`,
  `PlayerBeginConst`, `PlayerWaitConst`, `PlayerDie0Const`, `PlayerDie1Const`,
  `PlayerDie2Const`, `AppleManager`, `KniveManager`, `CheckPointManager`,
  `RedKeyManager`, `RockKeyManager`, `BladeManager`, `LockedDoorManager`,
  `PointManager`, `BonusManager`, `TouchTriggerManager`, `FrogManager`,
  `GiantAntManager`, `FirebirdManager`, `FlameManager`, `SnakeManager`,
  `CrabManager`, `GatorManager`, `SkullManager`, `GhostManager`, `BeesManager`,
  `HiveManager`, `EyesManager`, `LiftManager`, `RollingRockManager`,
  `CollapsingCeilingManager`, `FallingSpikeManager`, `BulletManager`,
  `BulletObjectFactory`, `HitFireManager`, `SwitchManager`, `ToggleWallManager`,
  `SparkManager`, `FirebirdPlayerManager`, `FirebirdWeaponManager`,
  `BubblesManager`, `UnderWaterRockManager`, `TextTileManager`,
  `HugeLetterTileManager`, `DemoMapManager`, `EpicManager`, `UtilityObjectEntity`,
  `SharedCode`, `ObjectSyncrhonizer`.
- `open-jill-object-background` (backgrounds): `StdBackgroundEntity`,
  `SpikeBackgroundEntity`, `KillLavaBackgroundEntity`, `KillWaterBackgroundEntity`,
  `Kill2BackgroundEntity`, `BaseTreeBackgroundEntity`, `BaseWaterBackgroundEntity`,
  `ShoreBackgroundEntity`, `BaseShoreBackgroundEntity`, `BaseCliffBackgroundEntity`,
  `MapDoorBackgroundEntity`, `MistBackgroundEntity`, `FFloorBackgroundEntity`,
  `FroofBackgroundEntity`, `WoodTorchBackgroundEntity`,
  `DoubleImageCopyRightBackgroundEntity`, `DoubleImageCopyTopBackgroundEntity`,
  `AbstractBaseBackgroundEntity`, `AbstractAnimateBackgroundEntity`,
  `AbstractOorBackgroundEntity`, `AbstractParameterBackgroundEntity`,
  `AbstractSynchronisedImageBackgroundEntity`.
- `openjill-core` (level execution): `AbstractExecutingStdPlayerLevel`,
  `AbstractObjectJillLevel`, `AbstractBackgroundJillLevel`. These drive the
  per-tick object and background update/draw loops.
- `OpenJill/src/main/resources`: `objects_manager_mapping.json` (type integer
  to manager class), `background_manager_mapping.properties` (DMA name string
  to background class).

Rust crates inspected for current state (all still stubs unless epic 5 is
merged first):

- `crates/openjill-core/src/lib.rs`: stub `CoreState`. Epic 5 will add
  `MessageDispatcher`, `MessageType`, `MessagePayload`, `ScreenHandler`,
  `RenderCommand`, `RuntimeState`, `InventoryObject`, `layout` module,
  `ScreenTransition`, `TickResult`, `SoundEvent`, `ActiveInput`.
- `crates/openjill-game/src/lib.rs`: stub `GameApp`. Epic 5 will add
  `GameOrchestrator`, `AssetCache`, `LevelConfig`, and screen handler
  implementations including the `LevelScreen` skeleton.
- `crates/openjill-data/src/lib.rs`: fully implemented parsers for DMA, SHA,
  JN, VCL, CFG.
- `crates/openjill-render/src/lib.rs`: stub `Renderer`. Epic 4/5 will add
  `Presenter` with `execute_and_present`.

**Prerequisite**: epic 5 must be merged before this epic starts. The `LevelScreen`
skeleton, `ScreenHandler::tick`, `RenderCommand`, `MessageDispatcher`, and
`RuntimeState` must all be stable.

## Pre-implementation audit required

Before starting any child issue, run:

```
openjill-rs dump jn --file JN1L01.JN1 --data-dir <OPENJILL_DATA_DIR>
openjill-rs dump jn --file JN1L02.JN1 --data-dir <OPENJILL_DATA_DIR>
```

for at least levels 1-3 and record which object types (integer `type` field in
`ObjectItem`) and background names (from DMA lookup) actually appear. Only
implement managers for types observed in episode 1 data; stub the rest with a
logged warning. The full type list in this document is exhaustive; the per-child
issue scope notes which types are expected to appear in episode 1.

## Required data files for manual checks

All committed tests use synthetic data. Real-data checks are gated by
`OPENJILL_DATA_DIR`.

- `JILL.DMA`: background name-to-tile metadata for all background entity
  rendering and collision classification.
- `JILL1.SHA`: all sprite and tile pixel data.
- `JILL1.VCL`: not directly used by gameplay, but `LevelScreen` init reads it
  for status bar text; already loaded in `AssetCache`.
- `JILL1.CFG`: not directly used by gameplay; already loaded in `AssetCache`.
- `JN1LNN.JN1` level files: object lists and background layers for episode 1
  levels. Required for integration checks.
- `MAP.JN1`: episode 1 world map. Already handled by `MapScreen` from epic 5;
  only `CheckPointManager` touches the map level sentinel.

## Object type registry

From `objects_manager_mapping.json`. Types without a manager receive a
`StubObjectEntity` that logs once and returns `None` from `msgDraw`.

| Type | Manager | Category |
|------|---------|----------|
| 0 | `PlayerEntity` | Player |
| 1 | `AppleEntity` | Pickup |
| 2 | `KnifeEntity` | Weapon pickup |
| 12 | `CheckPointEntity` | Level transition |
| 14 | `RedKeyEntity` | Key pickup |
| 15 | `TouchTriggerEntity` | Trigger |
| 20-21 | `TextTileEntity` | Decoration |
| 22 | `FrogEntity` | Enemy |
| 24 | `LockedDoorEntity` | Door |
| 25 | `CollapsingCeilingEntity` | Moving hazard |
| 26 | `ToggleWallEntity` | Interactive background |
| 27 | `PointEntity` | Score pickup |
| 28 | `BonusEntity` | Score pickup |
| 29 | `GiantAntEntity` | Enemy |
| 30 | `FirebirdEnemyEntity` | Enemy |
| 31 | `FlameEntity` | Hazard |
| 32 | `SwitchEntity` | Switch |
| 33 | `RockKeyEntity` | Key pickup |
| 35 | `RollingRockEntity` | Moving hazard |
| 36 | `BulletEntity` | Projectile |
| 37 | `HitFireEntity` | Projectile effect |
| 38 | `FallingSpikeEntity` | Moving hazard |
| 39 | `SnakeEntity` | Enemy |
| 40 | `UnderWaterRockEntity` | Underwater hazard |
| 42 | `HugeLetterTileEntity` | Decoration |
| 45 | `HiveEntity` | Enemy spawner |
| 46 | `BeesEntity` | Enemy swarm |
| 47 | `CrabEntity` | Enemy |
| 48 | `GatorEntity` | Enemy |
| 49 | `EpicEntity` | Stub |
| 50 | `BladeEntity` | Weapon |
| 51 | `SkullEntity` | Enemy |
| 53 | `GhostEntity` | Enemy |
| 56 | `FirebirdPlayerEntity` | Player form |
| 58 | `BubblesEntity` | Underwater FX |
| 61 | `LiftEntity` | Moving platform |
| 62 | `FirebirdWeaponEntity` | Projectile |
| 64 | `EyesEntity` | Enemy |
| 65 | `SparkEntity` | Effect |
| 67 | `DemoMapEntity` | Stub |

## Background entity registry

From `background_manager_mapping.properties`. The `default` entry applies to
any DMA name not explicitly listed.

| DMA name | Entity | Behavior |
|----------|--------|----------|
| (default) | `StdBackground` | Solid or passthrough per DMA flags |
| `MAPDOOR` | `MapDoorBackground` | Door tile on world map |
| `BASETREE` | `BaseTreeBackground` | Vine: player can climb |
| `BASEWATER` | `BaseWaterBackground` | Water surface |
| `SPIKE` | `SpikeBackground` | Kills player on touch |
| `SHORE` / `BASESHORE` | `ShoreBackground` / `BaseShoreBackground` | Shore decoration |
| `BASECLIFF` | `BaseCliffBackground` | Cliff decoration |
| `PILT` / `PILM` / `PILB` | `DoubleImageCopyRightBackground` | Pillar (double-wide) |
| `REDPLATL` / `REDPLATR` / `REDPLATM` | `DoubleImageCopyTopBackground` | Platform (double-tall) |
| `MIST` | `MistBackground` | Animated mist decoration |
| `FFLOOR` | `FFloorBackground` | Fake floor: player falls through from above |
| `FROOF` | `FroofBackground` | Fake roof: player passes through from below |
| `STALACBR` / `STALAGBR` / `WTHORN` / `WTHORN2` | `Kill2Background` | Kills player on touch |
| `LAVA1`-`LAVA5` | `KillLavaBackground` | Kills player (lava death type) |
| `WATERTL`/`WATERTR`/`WATERRD`/`WATERLD`/`WATERML`/`WATERMR` (and `*2`-`*4`) | `KillWaterBackground` | Kills player (water death type) |
| `WOODTORCH` | `WoodTorchBackground` | Animated torch decoration |

## Public interfaces, crate dependencies, and data types

### `openjill-core` additions

#### `ObjectEntity` trait

```rust
/// One active game object. Implemented per type by openjill-game.
pub trait ObjectEntity: Send {
    /// Advance state by one tick. May push messages to `dispatcher`.
    fn update(
        &mut self,
        input: &ActiveInput,
        state: &RuntimeState,
        backgrounds: &BackgroundGrid,
        dispatcher: &mut MessageDispatcher,
    );

    /// Return the render command for this object this tick, if visible.
    fn draw(&self) -> Option<RenderCommand>;

    /// Called when the player overlaps this object's bounding box.
    fn on_touch(&mut self, dispatcher: &mut MessageDispatcher);

    /// Called when a weapon hits this object.
    fn on_kill(&mut self, damage: i32, death_kind: DeathKind);

    /// Position and size (pixels) for collision and viewport culling.
    fn bounding_box(&self) -> Rect;

    /// True if this object should tick and draw when outside the viewport
    /// update border.
    fn always_active(&self) -> bool { false }

    /// True if this object acts as the checkpoint for level transitions.
    fn is_checkpoint(&self) -> bool { false }

    /// True if this is the player object.
    fn is_player(&self) -> bool { false }
}
```

#### `BackgroundEntity` trait

```rust
/// One background cell handler. Implemented per DMA name by openjill-game.
pub trait BackgroundEntity: Send {
    /// Return the render command for this cell (called once per visible cell
    /// per tick when `needs_update` is true).
    fn draw(&self, map_x: i32, map_y: i32) -> Option<RenderCommand>;

    /// Called each tick for cells that have dynamic behavior.
    fn update(&mut self, map_x: i32, map_y: i32, dispatcher: &mut MessageDispatcher);

    /// Called when the player's bounding box overlaps this cell.
    fn on_player_touch(&mut self, player: &mut dyn ObjectEntity, dispatcher: &mut MessageDispatcher);

    /// True if the player passes through this cell vertically (no floor).
    fn is_passthrough(&self) -> bool;

    /// True if the player can climb this cell (vine/tree).
    fn is_climbable(&self) -> bool;

    /// True if this cell behaves as a stair/slope.
    fn is_stair(&self) -> bool;

    /// True if this cell needs per-tick `update` calls.
    fn needs_update(&self) -> bool { false }
}
```

#### `BackgroundGrid` type

```rust
/// The full background layer for the current level.
/// Indexed as `grid[y][x]` where x in [0, 255] and y in [0, 63].
pub struct BackgroundGrid {
    pub cells: Vec<Vec<Box<dyn BackgroundEntity>>>,
    pub width: usize,   // 256
    pub height: usize,  // 64
}
```

#### `Rect` type

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn intersects(&self, other: &Rect) -> bool;
    pub fn contains_point(&self, px: i32, py: i32) -> bool;
}
```

#### `DeathKind` type

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeathKind {
    Enemy,
    Water,
    OtherBackground,
}
```

#### `InventoryObject` additions

Extend the `InventoryObject` enum (started in epic 5) to include all episode 1
inventory items:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InventoryObject {
    Jill,
    Gem,
    Key,      // RedKey / RockKey
    Knife,    // KniveManager
    Blade,    // BladeManager
    FireFlower,
    Firebird, // FirebirdPlayer form
}
```

### `openjill-game` additions

#### Object and background factories

```rust
/// Build the correct `ObjectEntity` implementation for a JN object type.
pub fn make_object_entity(type_id: u8, item: &ObjectItem, cache: &AssetCache)
    -> Box<dyn ObjectEntity>;

/// Build the correct `BackgroundEntity` implementation for a DMA name.
pub fn make_background_entity(dma_name: &str, map_code: u16, cache: &AssetCache)
    -> Box<dyn BackgroundEntity>;
```

Factories live in `crates/openjill-game/src/entities/`. Each entity type
gets its own file under `crates/openjill-game/src/entities/objects/` or
`crates/openjill-game/src/entities/backgrounds/`.

#### `LevelScreen` extensions (built on epic 5 skeleton)

The epic 5 `LevelScreen` skeleton loads JN object and background data.
This epic activates the per-tick update loop:

```
LevelScreen::tick:
  for each object (sorted by draw order):
    if object.always_active or within viewport update border:
      object.update(input, state, &backgrounds, &dispatcher)
    if within game area:
      if let Some(cmd) = object.draw():
        commands.push(cmd)
  for each visible background cell:
    bg.on_player_touch(player, &dispatcher) if player overlaps
    if bg.needs_update:
      bg.update(map_x, map_y, &dispatcher)
    commands.push(bg.draw(screen_x, screen_y))
  scroll viewport toward player if player near border
  process dispatcher message queue
  commands.extend(status_bar_dynamic_commands(state))
```

Player overlap with background cells: compute the set of cells overlapping
`player.bounding_box()` and call `on_player_touch` for each. The cell
coordinates come from dividing pixel positions by `BLOCK_SIZE = 16`.

Object-to-object collision: iterate all non-player, non-checkpoint objects
whose bounding box intersects the player bounding box and call `on_touch`.

#### Viewport scrolling

Scroll rules (from `AbstractExecutingStdPlayerLevel`):

- If `player.x < viewport_x + X_UPDATE_BORDER (96)`: scroll left.
- If `player.x + player.w > viewport_x + GAME_AREA_W - X_UPDATE_BORDER`:
  scroll right.
- If `player.y < viewport_y + Y_UPDATE_BORDER (48)`: scroll up.
- If `player.y + player.h > viewport_y + GAME_AREA_H - Y_UPDATE_BORDER`:
  scroll down.

Clamp viewport to `[0, MAP_WIDTH*16 - GAME_AREA_W]` x
`[0, MAP_HEIGHT*16 - GAME_AREA_H]`.

Background tiles blit at screen position
`(GAME_AREA_X + cell_x*16 - viewport_x, GAME_AREA_Y + cell_y*16 - viewport_y)`.

#### Player entity (`PlayerEntity`, type 0)

Rust translation of `PlayerManager` + `AbstractPlayerManager` +
`AbstractPlayerInteractionManager`. Lives in
`crates/openjill-game/src/entities/objects/player.rs`.

State machine:

| State | Sub-state meaning |
|-------|------------------|
| `Stand` | 0=face, 1=face-with-arm, can run left/right |
| `Still` | idle stand, no direction input |
| `Jumping` | 0=rising, 1=falling; xd/yd updated each tick |
| `Climbing` | 0=stop, 2=up; vertical movement only on vines |
| `Begin` | level-entry animation, 18 sub-state ticks |
| `Die` | sub-state = `DeathKind` index; colored bullet burst |

Movement constants (from const files):

- Stand run: `PLAYER_MOVE_SIZE = 8` px/tick left or right.
- Jump initial speed: `yd = -JUMP_INIT_SIZE = -16`; `xd` preserved from stand
  direction.
- Jump acceleration: `yd += JUMP_INCREMENT_VALUE = 2` each tick.
- Jump fall cap: `yd = min(yd, JUMP_FALLING_SPEED_LIMIT = 16)`.
- High jump (held Up): add `HIGH_JUMP_STEP_SIZE = 4` once at jump start.
- Climb up steps: `[0, 0, -6, -4, -4, -4]` px per tick sub-state.
- Climb down: `PLAYER_MOVE_SIZE_CLIMB_DOWN = 4` px/tick.
- Jump-from-climb: `yd = -JUMP_INIT_SIZE_FOR_CLIMB = -12`.
- Die0 (enemy/other): initial `yd = START_YD = -12`; spawn
  `NB_COLORED_BULLET = 10` colored `BulletEntity` objects.

Sprite tileset: index 8 for all player frames.

Tile indices (from const files):

| Sprite | Tile index |
|--------|-----------|
| Stand face-right | 20 |
| Stand face-left | 21 |
| Stand middle | 16 |
| Stand arm | 17 |
| Run-left frames | 8..15 |
| Fall | 60 |
| Duck/down | 61 |
| Jump middle | 56 |
| Jump left | 32 |
| Jump right | 40 |
| Climb 1-3 | 24, 25, 26 |
| Die (other) | 48..53 |
| Begin | PlayerBeginConst tiles |

`state_count` drives idle animations: raise arm at 154, show hint text at 254,
begin wait animation at 272, end at 301.

Hit-floor bounce: `state_count = 65529` triggers hit-floor animation
(sub-frames: `[36,20,37]` or `[44,21,45]` for left/right landing), ends after
`HIT_FLOOR_ANIMATION_COUNT_END = 5` frames.

Collision with background: use `UtilityObjectEntity.checkIfFloorUnderObject`
logic - check if the cell one pixel below the player bottom edge is solid
(`!is_passthrough`). If no floor: transition to `Jumping`. Horizontal
collision: if next position would overlap a solid cell, stop horizontal
movement.

Player fires weapon via `BulletObjectFactory` (see child issue 7).

### Workspace dependency additions

No new workspace crates expected. If a physics or geometry helper is needed,
add it to `openjill-core`. Do not introduce new workspace packages.

`serde_json` was conditionally added in epic 5; no additional dependencies
expected for this epic.

## Per-level sky / game-area background color

The Java reference port (`AbstractBackgroundJillLevel.loadLevel` and
`createBackgound`) takes the VGA color map shipped in
`sha-file/src/main/resources/jill_color_map.properties` and assigns
`defaultBackgroundColor = colorMap[0]`.  Palette index 0 in that table is
the `!000000` transparent sentinel, so the Java port composites the level
on top of a transparent fill that exposes whatever lies beneath in the
backing buffer (effectively black in the standard Swing presenter).  The
JN file format carries no sky-color field, the `JillLevelConfiguration`
class has no such member, and there is no derived per-level DMA palette
either.

Original DOS Jill of the Jungle episode 1 renders a saturated dark blue
sky (`0x0000A2`, VGA palette index 1) across all levels, which the Java
reference does *not* reproduce.  To match the original engine the Rust
port treats the sky color as a per-episode constant rather than a
per-level field:

- `crates/openjill-game/src/screens/level_screen.rs` exposes
  `pub const EPISODE_1_SKY_COLOR: u8 = 1;` and threads it through
  `LevelScreen::new` / `LevelScreen::from_bytes` as an explicit `sky_color`
  parameter.  The orchestrator passes `EPISODE_1_SKY_COLOR` at every
  construction site.
- `LevelScreen::render_base_frame` emits, in order: `RenderCommand::Clear`
  (the existing baseline), a `RenderCommand::FillRect` covering
  `(GAME_AREA_X, GAME_AREA_Y, GAME_AREA_W, GAME_AREA_H)` with
  `self.sky_color`, then the static background tile blits.  The presenter
  still clears the framebuffer to palette index 0 each frame; the
  per-level fill sits on top of that clear for the game-area region only,
  so the inventory / control / message-bar regions outside the game area
  remain unaffected.

When JN2 / JN3 episode support lands, replace the constant with an
episode-aware lookup (e.g., a function over the JN file extension or the
loaded episode descriptor) and update the orchestrator call sites to
forward the resolved value rather than the JN1-only constant.

## Status bar dynamic rendering

Epic 5 implements the static status bar tile mosaic. This epic drives dynamic
updates to:

- Score: decimal digits rendered as SHA font tiles at the control area
  score position. Update on every `InventoryPointMessage`.
- Lives: numeric icon at control area lives position. Update on every
  `InventoryLifeMessage`.
- Inventory icons: item icons at inventory area grid. Update on
  `InventoryItemMessage` add/remove.

Dynamic updates emit targeted `RenderCommand::FillRect` (erase) then
`RenderCommand::Blit` or `RenderCommand::DrawText` (redraw) for the changed
region only.

Status bar text (message bar at y=188): driven by `StatusBarTextMessage`.
Cleared after `LEVEL_MESSAGE_TICKS = 72` ticks (already implemented in
epic 5 for level transitions; reuse the same mechanism for gameplay messages).

## Tests and acceptance checks

All committed tests use synthetic data. Do not commit original game bytes.

Required tests by child issue (see child issues section):

- **Entity traits and factory (child 1)**: `make_object_entity` returns
  `StubObjectEntity` for an unregistered type without panic; returns correct
  concrete type for types 0, 1, 12. `Rect::intersects` returns true for
  overlapping rects and false for non-overlapping.
- **Player entity (child 2)**: with synthetic background grid (all solid floor),
  `Stand` state + jump input transitions to `Jumping` state; `Jumping` yd
  increases by 2 each tick; capped at 16. With no floor cell, standing player
  transitions to `Jumping`. With floor cell, jumping player with yd>0
  transitions to `Stand`. Climb input on vine cell transitions to `Climbing`;
  jump in `Climbing` transitions to `Jumping` with yd=-12. Die transition sets
  state to `Die` and pushes `DieRestartLevel` message after
  `STATECOUNT_MAX_TO_RESTART_GAME` ticks.
- **Viewport scrolling (child 3)**: viewport does not scroll when player is
  inside the update border; scrolls left when player x < viewport_x + 96;
  clamps at 0. Dynamic score update: `InventoryPointMessage` causes
  `RenderCommand::DrawText` for new score value.
- **Pickups and inventory (child 4)**: `AppleEntity::on_touch` pushes
  `InventoryItem` message with `InventoryObject::Gem`; `LockedDoorEntity`
  opens when `RuntimeState` inventory contains matching key type and removes
  the key; `CheckPointEntity` on_touch pushes `CheckpointChangeLevel` with
  correct level file; with no key, locked door `on_touch` is a no-op.
- **Kill backgrounds (child 5)**: `KillLavaBackground::on_player_touch`
  dispatches `DieRestartLevel` with `DeathKind::OtherBackground`;
  `KillWaterBackground::on_player_touch` dispatches `DieRestartLevel` with
  `DeathKind::Water`; `SpikeBackground::on_player_touch` kills player.
- **Enemies (child 6)**: `FrogEntity` transitions horizontal direction at the
  correct period; `FrogEntity::on_kill` with damage >= 1 marks it dead and
  stops updating; dead enemy produces no `RenderCommand`.
- **Projectiles and interactive objects (child 7)**: `BulletEntity` moves
  `xd/yd` px per tick and marks itself for removal after hitting a solid cell;
  `LiftEntity` moves player along with platform by pushing `PlayerMove` message;
  `SwitchEntity::on_touch` toggles `ToggleWallEntity` visibility.
- **Integration (gated by `OPENJILL_DATA_DIR`)**: `LevelScreen` loads episode
  1 level 1; one tick completes without panic; player `bounding_box()` is within
  the level bounds; score starts at `state.score` carry-in value.

Run `cargo test -p openjill-core` and `cargo test -p openjill-game` after each
child issue. Run `cargo test --workspace` and Taskfile lint checks before
marking the epic complete.

## Child issues

Implement in order. Each issue must reference this subplan file.

1. **Entity traits, `Rect`, factories, and `LevelScreen` update loop** -
   `ObjectEntity` and `BackgroundEntity` traits in `openjill-core`; `Rect`;
   `DeathKind`; `BackgroundGrid`; object and background factory functions in
   `openjill-game`; `StubObjectEntity` and `StdBackgroundEntity`; per-tick
   update loop wired into `LevelScreen::tick`.
2. **Player entity: movement, state machine, and collision** -
   Full `PlayerEntity` (type 0) in `openjill-game`: all states, movement
   constants, sprite selection, background collision, die burst.
3. **Viewport scrolling and dynamic status bar** - Player-driven viewport
   scroll in `LevelScreen`; dynamic score, lives, and inventory icon rendering.
4. **Pickups, keys, doors, inventory, and level exits** - `AppleEntity` (1),
   `KnifeEntity` (2), `PointEntity` (27), `BonusEntity` (28),
   `RedKeyEntity` (14), `RockKeyEntity` (33), `BladeEntity` (50),
   `LockedDoorEntity` (24), `CheckPointEntity` (12), `TouchTriggerEntity` (15);
   inventory item/life/point message dispatch; `InventoryObject` variants.
5. **Kill backgrounds and movement hazards** -
   `KillLavaBackground`, `KillWaterBackground`, `Kill2Background`,
   `SpikeBackground`; player death animation; `CollapsingCeilingEntity` (25),
   `FallingSpikeEntity` (38), `RollingRockEntity` (35), `FlameEntity` (31);
   `FFloorBackground`, `FroofBackground`, `BaseTreeBackground` (vine climb).
6. **Enemies** - `FrogEntity` (22), `GiantAntEntity` (29),
   `FirebirdEnemyEntity` (30), `SnakeEntity` (39), `CrabEntity` (47),
   `GatorEntity` (48), `SkullEntity` (51), `GhostEntity` (53),
   `HiveEntity` (45), `BeesEntity` (46), `EyesEntity` (64); basic movement,
   player collision damage, kill-on-weapon-hit; weapon `canFire` check.
7. **Projectiles, switches, and moving platform** -
   `BulletEntity` (36), `HitFireEntity` (37), `FirebirdWeaponEntity` (62);
   `BulletObjectFactory` (`PlayerEntity` fire logic);
   `SwitchEntity` (32), `ToggleWallEntity` (26), `LiftEntity` (61);
   `FirebirdPlayerEntity` (56), `BubblesEntity` (58), `SparkEntity` (65);
   decoration stubs for `TextTileEntity` (20-21), `HugeLetterTileEntity` (42).
8. **Integration: episode 1 playthrough verification** - Wire remaining stubs
   (`EpicEntity` 49, `UnderWaterRockEntity` 40, `DemoMapEntity` 67); run
   full episode 1 manually with original data; document any parity gaps;
   confirm `cargo test --workspace` passes; update AGENTS.md if new invariants
   were established.

## Known risks and handoff notes

- **`LevelScreen` skeleton stability**: the `ScreenHandler::tick` signature
  from epic 5 must not change after child issue 1. Agree on the final
  signature before merging epic 5.
- **Object draw order**: Java iterates objects in list order and draws
  backgrounds before objects. Preserve this order. Player is always drawn last
  among objects (checked against `AbstractExecutingStdLevel`).
- **Viewport update border vs. draw culling**: objects outside the 96/48 px
  border skip `update` but may still be drawn if within the game area. The
  border only gates ticking. Confirm this against `AbstractExecutingStdLevel`
  before child issue 1.
- **`checkIfFloorUnderObject` semantics**: in Java, `BackgroundEntity.isStair`
  returns `true` for sloped cells that count as floor. The Rust port should
  treat `is_stair` as equivalent to `!is_passthrough` for vertical collision
  unless the DMA dump shows a distinction. Verify before child issue 2.
- **`BackgroundGrid` cell resolution**: cells must be constructed from DMA
  data when `LevelScreen` loads a JN background layer, not lazily. The DMA
  lookup maps `map_code` to `(tileset, tile, flags, name)`. The `name` field
  selects the background entity class. Confirm the Rust DMA parser exposes
  the name string before child issue 1.
- **`InventoryObject` extend without breaking epic 5**: epic 5 defines a
  minimal `InventoryObject` enum. Child issue 4 extends it. Ensure the match
  in epic 5 code uses a wildcard arm or is updated when the enum grows.
- **`CheckPointManager` level-number encoding**: Java uses `SaveData.MAP_LEVEL
  = -1` as the world-map sentinel. Mirror as `MAP_LEVEL: i32 = -1` (already
  defined in epic 5 `RuntimeState`). `CheckPointEntity` sets this when the
  checkpoint's counter equals the level number for map-return.
- **`LiftEntity` and `PlayerMove` message**: lifts move the player by
  dispatching `PlayerMove` through the message bus, not by directly mutating
  `RuntimeState`. The player entity must subscribe to `PlayerMove` during init
  and apply the delta to its position.
- **`FirebirdPlayer` form**: type 56 is a second player entity active when Jill
  transforms. Only one player entity ticks as `is_player = true` at a time.
  `ChangePlayerCharacter` message switches the active player reference.
  Implement in child issue 7; can be a stub that logs in earlier issues.
- **Die burst bullets**: `PlayerDie0Const.NB_COLORED_BULLET = 10` colored
  bullets are spawned via `CreateObject` messages. `BulletObjectFactory` must
  exist before child issue 5 can implement die correctly. Stub it until child
  issue 7 lands `BulletEntity`.
- **`FFLOOR` / `FROOF` semantics**: `FFloorBackground` allows the player to
  pass through from above (floor exists when approaching from below only);
  `FroofBackground` is the ceiling equivalent. Confirm via `AbstractBaseBackgroundEntity`
  `isPlayerThru` / `isStair` before child issue 5.
- **`Kill2Background` (STALACBR/STALAGBR/WTHORN/WTHORN2)**: kills with
  `DeathKind::OtherBackground`, not water. Keep separate from lava kill type
  as the player die animation differs (die sub-state 0 vs. 2).
- **SHA tileset identity for enemies**: each enemy uses a specific tileset and
  tile range from SHA. Verify tileset indices by dumping SHA with
  `openjill-rs dump sha` before child issue 6. Do not hard-code tileset numbers
  without verification.
- **`zaphold` field**: after an object touches the player, `zaphold` is set to
  `ZAPHOLD_AFTER_TOUCH = 3` on the object. While non-zero, the object skips
  collision with the player. Decrement once per tick. Implement in
  `AbstractHitPlayerObjectEntity` equivalent in Rust, shared across all enemy
  types.
- **Object removal**: objects push `ObjectListMessage` (remove self) to the
  dispatcher when they die or are collected. The `LevelScreen` update loop
  must drain these messages and remove matching objects after each tick pass.
  Do not remove inside the iteration.
- **`forbid(unsafe_code)`**: maintain this attribute in all crates it already
  covers.
- Add doc comments for every new module, type, field, function, and method per
  `AGENTS.md`.
