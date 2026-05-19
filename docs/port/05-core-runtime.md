# Epic 5 Subplan: Core Runtime Flow

## Inspected OpenJill modules and Rust crates

Java reference modules inspected for behavior:

- `openjill-core-api`: `EnumMessageType`, `MessageDispatcher`,
  `InterfaceMessageGameHandler`, `EnumScreenType`, `EnumInventoryObject`,
  `JillConst`, `TileManager`, `TextManager`, and all message payload types.
  Source of all inter-system message contracts.
- `openjill-core` (level hierarchy): `AbstractBasicCacheLevel`,
  `AbstractBackgroundJillLevel`, `AbstractObjectJillLevel`,
  `AbstractMenuJillLevel`, `AbstractExecutingStdLevel`,
  `AbstractExecutingStdPlayerLevel`, `AbstractChangeLevel`. Together these define
  the asset-loading, background, object, menu, tick, player, and
  level-transition layers. The Rust port collapses this into `ScreenHandler`
  implementations rather than a deep inheritance chain.
- `openjill-core` (screen handlers for Jill 1): `StartMenuJill1Handler`,
  `MapLevelHandler`, `LoadNewLevelHandler`, `StoryScreenJill1Handler`,
  `CreditScreenJill1Handler`, `OrderingInfoScreenJill1Handler`,
  `NoisemakerScreenJill1Handler`. These define the episode-1 screen flow.
- `openjill-core` (message dispatcher): `MessageDispatcherImpl`. Maps
  `EnumMessageType` to subscriber lists; queues messages sent before any
  subscriber exists and delivers them on first subscription.
- `openjill-core` (screen and GUI): `StatusBar`, `ControlArea`, `InventoryArea`,
  `LevelMessageBox`, `InformationBox`, `AbstractMenu`, `ClassicMenu`,
  `HighScoreMenu`. Define the VGA screen layout and overlay rendering.
- `openjill-core` (config): `JillLevelConfiguration`, `LevelConfiguration`,
  `JillGameConfig`. Level configuration bundle: SHA name, optional JN file name,
  VCL name, CFG name, save prefix, start screen class, level number, carry-in
  score and gem count, optional in-memory map bytes.
- `openjill-core-api` (`JillConst`): block size = 16 px,
  `xUpdateScreenBorder` = 96 px (6 blocks), `yUpdateScreenBorder` = 48 px (3
  blocks), `zapholdValueAfterTouchPlayer` = 3.
- `OpenJill/src/main/resources`: `open_jill.properties` (55 ms tick delay,
  320x200, VGA default, 4000 ms level-message timeout, entry point =
  `StartMenuJill1Handler`), `status_bar_vga.json` (VGA layout: game area
  x=80 y=16 w=232 h=160, control area x=8 y=16 w=64 h=85, inventory area
  x=8 y=107 w=64 h=69, message bar y=188 h=12), `start_menu.json` (menu
  position, tile references, item list with values 0/1/2/3/4/5/7/9),
  `level_messagebox_vga.json`, `information_box.json`, `control_area.json`,
  `inventory_conf.json`.

Rust crates inspected for current state:

- `crates/openjill-core/src/lib.rs`: stub `CoreState` wrapping `DataDirectory`.
- `crates/openjill-game/src/lib.rs`: stub `GameApp` holding `CoreState`,
  `Renderer`, `AudioBackend`.
- `crates/openjill-render/src/lib.rs`: exists but may still be a stub; epic 4
  owns `Presenter`.

## Required data files for manual checks

These files are required only for integration checks gated by
`OPENJILL_DATA_DIR`. All committed tests use synthetic data.

- `JILL.DMA`: background map-code to tileset/tile/flags lookup. Required for
  background rendering.
- `JILL1.SHA`: all graphics data. Required for status bar, menu box tiles,
  inventory icons, and background tiles.
- `JILL1.VCL`: VCL text entries. Entry 0 is the instructions text shown by the
  info box on the start menu.
- `JILL1.CFG`: high scores and save slots. Required for the high-score overlay
  and save/load menus.
- `INTRO.JN1`: contains the intro, story, ordering-info, credits,
  noisemaker, and start-menu screens as background/object layers.
- `MAP.JN1`: episode 1 world map. Required for map loading.
- Level files `*.JN1` via the `JN1` save prefix (e.g. `JN1L01.JN1`,
  `JN1L02.JN1`, ...). Required for level-transition testing.

## Public interfaces, crate dependencies, and data types

### `openjill-core` additions

#### `RenderCommand` (deferred from epic 4)

```rust
/// One rendering instruction produced by a screen handler per tick.
/// The renderer executes these in order on its framebuffer.
pub enum RenderCommand {
    /// Clear the framebuffer to palette index `color`.
    Clear { color: u8 },
    /// Blit a tile from SHA tileset/tile at (x, y). Pixel 0 is transparent
    /// unless `opaque` is true.
    Blit {
        tileset: u8,
        tile: u16,
        x: i32,
        y: i32,
        opaque: bool,
    },
    /// Draw a text string at (x, y) using the SHA font tileset.
    DrawText {
        text: String,
        x: i32,
        y: i32,
        color_index: u8,
    },
    /// Fill a rectangle with palette index `color`.
    FillRect {
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        color: u8,
    },
}
```

`RenderCommand` lives in `openjill-core` with no GPU imports. `openjill-render`
receives a `&[RenderCommand]` from `openjill-game` each tick and executes them.

#### Message dispatcher

```rust
/// Message types that cross subsystem boundaries.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MessageType {
    PlayerMove,
    PlayerGetPosition,
    InventoryItem,
    InventoryLife,
    InventoryPoint,
    StatusBarText,
    Object,
    ReplaceObject,
    CreateObject,
    Trigger,
    Background,
    CheckpointChangeLevel,
    CheckpointChangeLevelPrevious,
    DieRestartLevel,
    MessageBox,
    ChangePlayerCharacter,
}

/// Payload for a level-change checkpoint.
pub struct ChangeLevelPayload {
    pub level_file: String,  // JN filename, e.g. "JN1L03.JN1"
    pub level_number: i32,   // level index; SaveData::MAP_LEVEL = -1
}

/// Dispatcher stores subscribers per type and queues messages sent before
/// any subscriber exists.
pub struct MessageDispatcher { … }

impl MessageDispatcher {
    pub fn new() -> Self;
    pub fn subscribe(&mut self, msg_type: MessageType, handler: Box<dyn MessageHandler>);
    pub fn send(&mut self, msg_type: MessageType, payload: MessagePayload);
    pub fn clear(&mut self);
}

pub trait MessageHandler: Send {
    fn handle(&mut self, msg_type: MessageType, payload: &MessagePayload);
}
```

`MessagePayload` is an enum covering all payload types. Queued messages
(sent before any subscriber) are flushed when the first subscriber registers
for that type, matching `MessageDispatcherImpl` behavior.

#### Screen and layout constants

```rust
pub mod layout {
    /// Native game resolution.
    pub const SCREEN_WIDTH: u32 = 320;
    pub const SCREEN_HEIGHT: u32 = 200;

    /// VGA layout (from status_bar_vga.json).
    pub const GAME_AREA_X: i32 = 80;
    pub const GAME_AREA_Y: i32 = 16;
    pub const GAME_AREA_W: u32 = 232;
    pub const GAME_AREA_H: u32 = 160;

    pub const CONTROL_AREA_X: i32 = 8;
    pub const CONTROL_AREA_Y: i32 = 16;
    pub const CONTROL_AREA_W: u32 = 64;
    pub const CONTROL_AREA_H: u32 = 85;

    pub const INVENTORY_AREA_X: i32 = 8;
    pub const INVENTORY_AREA_Y: i32 = 107;
    pub const INVENTORY_AREA_W: u32 = 64;
    pub const INVENTORY_AREA_H: u32 = 69;

    pub const MESSAGE_BAR_Y: i32 = 188;
    pub const MESSAGE_BAR_H: u32 = 12;

    /// Tile/block size in pixels.
    pub const BLOCK_SIZE: u32 = 16;

    /// Screen-update border (objects outside this box relative to viewport
    /// skip update).
    pub const X_UPDATE_BORDER: u32 = 96;   // 6 blocks
    pub const Y_UPDATE_BORDER: u32 = 48;   // 3 blocks

    /// Level-message display duration in ticks (4000 ms / 55 ms).
    pub const LEVEL_MESSAGE_TICKS: u32 = 72;

    /// Zaphold value placed on objects after touching the player.
    pub const ZAPHOLD_AFTER_TOUCH: u32 = 3;
}
```

#### Runtime state

```rust
/// Persistent state carried across screen transitions.
pub struct RuntimeState {
    pub level: i32,          // current level number; MAP_LEVEL = -1
    pub score: i32,
    pub lives: i32,
    pub gem_count: i32,
    pub inventory: Vec<InventoryObject>,
}

pub const MAP_LEVEL: i32 = -1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InventoryObject {
    Jill,
    Gem,
    Key,
    FireFlower,
    // extend as gameplay epic requires
}
```

#### Screen handler trait

```rust
/// One loaded screen. Implemented by each screen type.
pub trait ScreenHandler {
    /// Advance state by one fixed tick. Returns a list of render commands
    /// and optionally a transition to a new screen.
    fn tick(
        &mut self,
        input: &ActiveInput,
        state: &mut RuntimeState,
    ) -> TickResult;
}

pub struct TickResult {
    pub commands: Vec<RenderCommand>,
    pub transition: Option<ScreenTransition>,
    pub sound_events: Vec<SoundEvent>,
}

pub enum ScreenTransition {
    StartMenu,
    Map,
    Level { file: String, number: i32 },
    RestartLevel,
    Story,
    Credits,
    OrderingInfo,
    Noisemaker,
    Quit,
}
```

`ActiveInput` is defined in `openjill-core` from epic 4 (`BTreeSet<InputCommand>`
or equivalent active-key set).

### `openjill-game` additions

#### Asset cache

```rust
/// Loaded and cached file data for one episode.
pub struct AssetCache {
    pub dma: DmaFile,
    pub sha: ShaFile,
    pub vcl: VclFile,
    pub cfg: CfgFile,
}

impl AssetCache {
    /// Load all required assets from `data_dir`. Fails fast if any required
    /// file is missing.
    pub fn load(data_dir: &DataDirectory) -> Result<Self, AssetError>;
}
```

The cache loads once at application start. Individual screens borrow from it;
they do not own or re-load it. JN files are loaded per-screen transition because
each screen has a different JN file.

#### `LevelConfig`

```rust
/// Configuration bundle passed to a screen handler on construction.
pub struct LevelConfig {
    pub jn_file: Option<String>,    // None for screens without a JN file
    pub level_number: i32,
    pub start_screen: ScreenTransition,  // where Escape/Quit returns
    pub carry_score: i32,
    pub carry_gems: i32,
    pub map_jn_bytes: Option<Vec<u8>>,   // in-memory map data for save/load
    pub level_jn_bytes: Option<Vec<u8>>, // in-memory level for restart
}
```

#### `GameOrchestrator`

```rust
/// Owns the current screen handler. Called each tick by the winit event loop.
pub struct GameOrchestrator {
    cache: AssetCache,
    state: RuntimeState,
    handler: Box<dyn ScreenHandler>,
    data_dir: DataDirectory,
}

impl GameOrchestrator {
    pub fn new(data_dir: DataDirectory) -> Result<Self, OrchestratorError>;

    /// Called from `about_to_wait` after the 55 ms tick fires.
    pub fn tick(&mut self, input: &ActiveInput) -> Vec<RenderCommand>;

    /// Apply a `ScreenTransition` returned by `tick`.
    fn apply_transition(&mut self, transition: ScreenTransition);
}
```

`apply_transition` constructs the next `ScreenHandler` from `LevelConfig` and
the asset cache. It serializes the current map JN bytes into an in-memory buffer
before releasing the current handler (mirroring `putCurrentLevelInFileMemory`).

#### Screen handler implementations

Each handler lives in `crates/openjill-game/src/screens/`:

| Handler | JN file | Description |
|---------|---------|-------------|
| `StartMenuScreen` | `INTRO.JN1` | Main menu; high-score overlay on exit |
| `StoryScreen` | `INTRO.JN1` | Story text screen; auto-advance or key |
| `CreditsScreen` | `INTRO.JN1` | Credits screen |
| `OrderingInfoScreen` | `INTRO.JN1` | Ordering information screen |
| `NoisemakerScreen` | `INTRO.JN1` | Noisemaker/demo screen |
| `MapScreen` | `MAP.JN1` | Episode 1 world map |
| `LevelScreen` | `JN1LNN.JN1` | A playable level |

All INTRO.JN1-based screens share the same loaded JN bytes; only the
background-layer offset and object selection differ per screen. Determine the
exact per-screen offsets by inspecting `INTRO.JN1` with `openjill-rs dump jn`.

#### Start menu flow

Based on `StartMenuJill1Handler` and `start_menu.json`:

| Menu item value | Action |
|---|---|
| 0 (play) | `ScreenTransition::Map` |
| 1 (restore) | Open load-game overlay |
| 2 (story) | `ScreenTransition::Story` |
| 3 (instructions) | Show info box (VCL text entry 0) |
| 4 (ordering info) | `ScreenTransition::OrderingInfo` |
| 5 (credits) | `ScreenTransition::Credits` |
| 7 (noisemaker) | `ScreenTransition::Noisemaker` |
| 9 (quit) | `ScreenTransition::Quit` |

Menu item value 6 (demo) is present in the JSON but not handled in
`StartMenuJill1Handler`; skip for this epic.

Escape from start menu exits the application (matches Java `System.exit(0)`).

`centerScreen` in `StartMenuJill1Handler` sets the background offset to
`-(112+1)*16, -(53+1)*16` = `(-1808, -864)`. The VGA game area is 232x160
px; the background layer is `MAP_WIDTH * BLOCK_SIZE = 256 * 16 = 4096` px wide
and `MAP_HEIGHT * BLOCK_SIZE = 64 * 16 = 1024` px tall. Clamp the offset so
only the relevant region blits into the game area.

### `openjill-render` additions

`Presenter` already exposes `clear`, `blit`, `draw_text`, and `present` from
epic 4. This epic adds the `RenderCommand` executor:

```rust
impl Presenter {
    /// Execute a slice of render commands against the internal framebuffer,
    /// then call `present`.
    pub fn execute_and_present(
        &mut self,
        commands: &[RenderCommand],
        sha: &ShaFile,
        palette: &Palette,
    ) -> Result<(), PresenterError>;
}
```

`execute_and_present` resolves each `RenderCommand::Blit { tileset, tile, … }`
by looking up the indexed pixel data from `sha`, then calls `self.blit`. It is
the only place where `ShaFile` tile data is turned into framebuffer pixels.

### `openjill-game` event loop wiring

The winit `GameApp::about_to_wait` handler:

```
if elapsed >= 55ms:
    commands = orchestrator.tick(&active_input)
    presenter.execute_and_present(&commands, &cache.sha, &palette)
    reset tick timer
else:
    presenter.execute_and_present(&last_commands, &cache.sha, &palette)
```

Caching `last_commands` allows `present` every vsync while ticking at 18 Hz,
matching epic 4's frame/tick separation.

### Workspace dependency additions

No new workspace dependencies are expected for this epic. All required crates
(`wgpu`, `winit`, `bytemuck`, `thiserror`, `pollster`) were added in epic 4.
If `serde_json` is needed to load JSON resource files at runtime, add
`serde_json = "1"` and `serde = { version = "1", features = ["derive"] }` to
`[workspace.dependencies]`. Prefer embedding JSON as `include_str!` and parsing
once at startup over runtime file reads for the resource files.

## Status bar and screen layout

The VGA status bar is a static tile mosaic drawn from SHA tileset 3. Tile
indices come from `status_bar_vga.json` (the `images` array). The Rust
implementation renders the status bar as a fixed set of `RenderCommand::Blit`
calls emitted once on screen entry, then re-emitted only on state changes.

The status bar is drawn to the full 320x200 framebuffer before the game area.
The game area begins at x=80, y=16. Background tiles from the JN file blit into
the game area only; they must be clipped to `[GAME_AREA_X .. GAME_AREA_X +
GAME_AREA_W, GAME_AREA_Y .. GAME_AREA_Y + GAME_AREA_H]`.

Score, lives, and inventory icons update in the control area (x=8, y=16) and
inventory area (x=8, y=107). For this epic, render static placeholders for
score/lives; the gameplay epic will drive dynamic updates.

The message bar (y=188, h=12) displays level-transition text. The Java timeout
is 4000 ms = 72 ticks at 55 ms. Decrement a counter each tick; clear the bar
when it reaches zero.

## JN file background rendering

A JN background layer is `MAP_WIDTH * MAP_HEIGHT = 256 * 64 = 16 384` cells,
each storing a `u16le` map code. Each map code is looked up in `DmaFile` to
obtain `(tileset, tile, flags)`. The tile pixel data comes from `ShaFile`.

For map and level screens, the viewport scrolls within the background. The
viewport offset (in pixels) determines which background cells are visible.
Render only cells that overlap the game area (`GAME_AREA_X`..`+GAME_AREA_W`,
`GAME_AREA_Y`..`+GAME_AREA_H`), offsetting each cell's blit position by
`(GAME_AREA_X - viewport_x % BLOCK_SIZE, GAME_AREA_Y - viewport_y % BLOCK_SIZE)`.

For this epic, implement static background rendering only (viewport fixed at the
initial offset from `centerScreen`). The gameplay epic drives scrolling.

## Tests and acceptance checks

Use synthetic data for all committed tests. Do not commit original game bytes.

Required tests by child issue:

- **Message dispatcher**: `send` before `subscribe` queues the payload;
  `subscribe` after delivers queued payloads; `send` with a subscriber delivers
  immediately; `clear` removes all subscribers; multiple subscribers on one type
  all receive the payload.
- **`RenderCommand` and layout constants**: `RenderCommand` variants serialize
  and reconstruct correctly (if `serde` is added); layout constants match the
  JSON values in `status_bar_vga.json`; `execute_and_present` calls `blit` for
  each `Blit` command with a synthetic SHA and verifies the framebuffer position.
- **Asset cache**: `AssetCache::load` returns a descriptive error when a
  required file is absent; with synthetic fixtures for each format it constructs
  without error.
- **`GameOrchestrator` tick**: a synthetic `StartMenuScreen` that immediately
  returns `ScreenTransition::Map` causes `apply_transition` to swap in a
  `MapScreen`; `tick` returns commands from the new handler on the following
  call.
- **Start menu screen**: key-press value 0 produces `ScreenTransition::Map`;
  value 9 produces `ScreenTransition::Quit`; Escape produces
  `ScreenTransition::Quit` (not map); info-box overlay appears on value 3.
- **Map screen**: synthetic `MAP.JN1` bytes (minimal valid JN: background layer
  of 16 384 zero map codes, zero objects, minimal save data) load without error;
  `tick` returns at least one `RenderCommand::Clear`; a non-zero map code that
  resolves to a known tileset/tile via a synthetic DMA produces a `Blit` command.
- **Level loading and transitions**: `CHECK_POINT_CHANGING_LEVEL` message causes
  `GameOrchestrator` to load a new `LevelScreen` after `LEVEL_MESSAGE_TICKS`
  ticks; `CHECK_POINT_CHANGING_LEVEL_PREVIOUS` returns to `MapScreen`;
  `DIE_RESTART_LEVEL` restarts the current level.
- **Integration** (gated by `OPENJILL_DATA_DIR`): `AssetCache::load` succeeds
  with real episode 1 files; `StartMenuScreen::new` produces render commands
  that include at least one `Blit`; one full tick completes without error or
  panic.

Run `cargo test -p openjill-core` and `cargo test -p openjill-game` during each
child issue. Run `cargo test --workspace` and the Taskfile lint checks before
marking the epic complete.

## Child issues

Implement in order:

1. **`RenderCommand` enum + message dispatcher** - `openjill-core` additions;
   no IO, no assets. Reference this subplan file.
2. **Asset cache, `LevelConfig`, and `GameOrchestrator` skeleton** -
   `openjill-game` orchestration layer; `AssetCache::load` and tick plumbing.
   Reference this subplan file.
3. **Status bar and screen layout** - static VGA status bar tile mosaic;
   `execute_and_present` in `openjill-render`. Reference this subplan file.
4. **Start menu and intro/special screens** - `StartMenuScreen` and the five
   secondary screens backed by `INTRO.JN1`. Reference this subplan file.
5. **Map loading and static background rendering** - `MapScreen` with fixed
   viewport from `MAP.JN1`. Reference this subplan file.
6. **Level loading and screen transitions** - `LevelScreen` skeleton, level
   message box timer, `CHECK_POINT_*` and `DIE_RESTART_LEVEL` message
   handling. Reference this subplan file.

## Known risks and handoff notes

- **`INTRO.JN1` screen offsets**: the Java code uses `centerScreen()` only in
  `StartMenuJill1Handler`; other INTRO screens use different offsets. Determine
  exact offsets for each screen by dumping `INTRO.JN1` with `openjill-rs dump
  jn` before starting child issue 4. Do not guess.
- **`RenderCommand` backward reach into epic 4**: epic 4 left `RenderCommand`
  deferred. This epic defines it; the `execute_and_present` addition to
  `Presenter` is a backward touch of the render crate. Coordinate with epic 4
  branch state before merging.
- **JSON resource loading**: `start_menu.json`, `status_bar_vga.json`,
  `level_messagebox_vga.json`, and `information_box.json` are in
  `OpenJill/src/main/resources`. Decide in child issue 1 whether to embed them
  as `include_str!` constants in Rust or load them from disk at runtime. Embedded
  is simpler and avoids a runtime file dependency; prefer it unless configurability
  is required.
- **`DmaFile` map-code 0**: map code 0 is the transparent/empty cell in OpenJill.
  Confirm via `openjill-rs dump dma` whether entry 0 has a valid tile or is a
  sentinel. Skip rendering for transparent cells.
- **SHA tileset 3 identity**: the status bar tiles come from SHA tileset index 3
  (0-based). Verify this against the real SHA file before child issue 3. Tileset
  numbering is 0-indexed in the Rust parser; confirm the Java parser agrees.
- **`INTRO.JN1` object layer**: `StartMenuJill1Handler` constructs
  `ClassicMenu` from `start_menu.json` and a reference to `shaFile`. The menu
  box is drawn from SHA tileset 7 tiles. The Rust port emits these as
  `RenderCommand::Blit` entries rather than using a menu object hierarchy.
- **Save/load menu**: the start menu's "restore" option opens a load-game
  overlay driven by `JILL1.CFG` save slots. This epic implements the overlay
  structure and CFG read path. Actual save-file writing is owned by epic 7
  (Save/Config); do not implement write paths here.
- **`MAP_WIDTH` and `MAP_HEIGHT`**: Java `BackgroundLayer.MAP_WIDTH = 256`,
  `MAP_HEIGHT = 64`. Confirm these match the Rust parser's `BackgroundLayer`
  struct before child issue 5.
- **Level number encoding**: `SaveData.MAP_LEVEL` is `-1` in Java. Mirror this
  as `const MAP_LEVEL: i32 = -1` in Rust; use it as the sentinel throughout.
- **Handoff to gameplay epic (6)**: this epic leaves entity behavior minimal -
  objects are loaded from JN files but do not tick. The gameplay epic attaches
  behavior by implementing `ObjectEntity` update logic. The `ScreenHandler::tick`
  signature must be stable before the gameplay epic starts; do not change it
  after child issue 1.
- **`forbid(unsafe_code)`**: maintain this attribute in all crates it already
  covers. Do not remove it to work around `serde_json` or any other crate usage.
- Add doc comments for every new module, type, field, function, and method per
  `AGENTS.md`.
