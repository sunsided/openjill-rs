# Epic 4 Subplan: wgpu Rendering and winit Input

## Inspected OpenJill modules and Rust crates

This phase builds directly on the parsed data from `docs/port/02-original-data-parsers.md`
and the debug inspection tools from `docs/port/03-asset-debug-tools.md`.

Java reference modules inspected for behavior:

- `simplegame`: Java Swing game loop, keyboard listener, `JillCanvas`, and
  `GameDisplay`. Provides the authoritative timing constant (`55 ms` per tick),
  the 320x200 framebuffer contract, and the keyboard table used for input
  mapping. Replace with Rust; do not port Swing code.
- `sha-file` / `sha-file-api`: color-map structure, indexed pixel layout,
  per-image `type` field, and `is_font` flag. Required to blit tiles and render
  text.
- `openjill-core-api`: `AbstractGameObject`, `CacheManager`, `TileManager`,
  and `ScreenType` interfaces. Required to understand the render-command and
  sound-event boundaries the renderer must respect.
- `openjill-core`: `JillMain`, `AbstractLevel`, and screen handler startup
  flow. Required to understand what the renderer receives rather than what it
  decides.
- `OpenJill/src/main/resources`: `controls.properties` keyboard table. Used as
  the canonical key-to-action mapping source.

Rust crates that this phase expects to touch:

- `crates/openjill-render`: implement `wgpu` device setup, swap chain, indexed
  framebuffer, RGBA upload, nearest-neighbor scaling, sprite/tile blitting,
  font text drawing, and the `Presenter` public type.
- `crates/openjill-core`: add `InputCommand` enum, `RenderCommand` enum
  (optional for this phase), and `Palette` type. No windowing or GPU code here.
- `crates/openjill-game`: add event loop wiring that feeds winit key events
  into `InputCommand` values and drives the tick + present cycle.
- `crates/openjill-cli`: extend the `run` stub to start the winit event loop.
- `Cargo.toml` (workspace): add `wgpu`, `winit`, and `bytemuck` to
  `[workspace.dependencies]`.

Do not add `wgpu`, `winit`, or any GPU crate as a dependency of
`openjill-data`, `openjill-core`, or `tools/openjill-dump`. The renderer must
remain absent from pure logic and parser crates.

## Required data files for manual checks

These files are required only for optional real-data integration checks gated by
`OPENJILL_DATA_DIR`. All committed tests use synthetic data.

- `JILL1.SHA`: source of all tile, sprite, font, and picture indexed pixel data
  and color maps. Required to test real blitting and palette expansion.
- `JILL.DMA`: map-code to tileset/tile index mapping. Required to verify tile
  lookup during any level rendering work that falls within this epic.

No other original data files are required for the rendering and input layer
alone.

## Public interfaces, crate dependencies, and data types

### `openjill-core` additions

```rust
/// One logical input action. Produced by the input layer, consumed by game logic.
pub enum InputCommand {
    MoveLeft,
    MoveRight,
    Jump,
    Duck,
    ThrowItem,
    NextInventory,
    PrevInventory,
    Pause,
    Quit,
}

/// One 256-color VGA palette. Entries are 6-bit RGB values as returned by the
/// SHA color map; call `Palette::expand_6bit` to get 8-bit RGBA bytes.
pub struct Palette {
    pub entries: [[u8; 3]; 256],
}

impl Palette {
    /// Build from SHA color-map entries. Expands 6-bit values to 8-bit.
    pub fn from_sha_color_map(entries: &[ShaColorMapEntry]) -> Self { … }

    /// Return the RGBA bytes for one indexed pixel.
    pub fn rgba(&self, index: u8) -> [u8; 4] { … }
}
```

`RenderCommand` is deferred to the Core Runtime epic (5) because it requires
knowledge of screen layout. This epic does not need a command list; the
framebuffer is driven directly by blitting calls from game orchestration code.

### `openjill-render` public surface

```rust
/// Owns the wgpu device, surface, swap chain, and the 320x200 indexed
/// framebuffer. Create once; call methods from the winit event loop.
pub struct Presenter { … }

impl Presenter {
    /// Async-capable constructor. Resolves the adapter and device.
    pub async fn new(window: Arc<Window>) -> Result<Self, PresenterError>;

    /// Resize the swap chain when the window dimensions change.
    pub fn resize(&mut self, width: u32, height: u32);

    /// Clear the indexed framebuffer to `color_index`.
    pub fn clear(&mut self, color_index: u8);

    /// Blit a tile from indexed pixel data into the framebuffer at (x, y).
    /// Pixels with index 0 are treated as transparent unless `opaque` is true.
    pub fn blit(
        &mut self,
        src: &[u8],
        src_width: u8,
        src_height: u8,
        dst_x: i32,
        dst_y: i32,
        opaque: bool,
    );

    /// Draw a text string using the SHA font tileset at (x, y).
    pub fn draw_text(&mut self, text: &str, x: i32, y: i32, font: &ShaFontTiles);

    /// Expand the indexed framebuffer through `palette`, upload to GPU, and
    /// present the frame. Returns an error if the surface is lost.
    pub fn present(&mut self, palette: &Palette) -> Result<(), PresenterError>;
}

pub struct ShaFontTiles { … }   // wraps glyph index data extracted from SHA

#[derive(Debug, thiserror::Error)]
pub enum PresenterError { … }
```

### `openjill-game` additions

```rust
pub struct GameApp {
    data_dir: PathBuf,
    presenter: Option<Presenter>,   // populated after window creation
    palette: Palette,
    pending_commands: Vec<InputCommand>,
}

impl ApplicationHandler for GameApp { … }  // winit 0.30 trait
```

### Workspace dependency additions

```toml
bytemuck = { version = "1", features = ["derive"] }
thiserror = "2"
wgpu = "24"
winit = "0.30"
```

`bytemuck` is used to cast the expanded RGBA buffer to `&[u8]` for the wgpu
texture write. `thiserror` is used for `PresenterError`. Both should be added to
`[workspace.dependencies]`.

Avoid pulling in `tokio` for this phase. Use `pollster` to block on the async
`Presenter::new` call inside the winit `resumed` handler, or restructure
initialization as non-async. Add `pollster = "0.3"` to workspace dependencies
if needed.

## wgpu setup

### Adapter and device selection

```
EventLoop resumed
  -> create winit Window
  -> wgpu::Instance::new(InstanceDescriptor { backends: Backends::all(), … })
  -> instance.create_surface(&window)
  -> instance.request_adapter(&RequestAdapterOptions {
         power_preference: PowerPreference::None,
         compatible_surface: Some(&surface),
         force_fallback_adapter: false,
     })
  -> adapter.request_device(&DeviceDescriptor::default(), None)
  -> Presenter::new stores: device, queue, surface, surface_config
```

Fail fast if no compatible adapter is found. Print the error and exit; do not
silently fall back to a software renderer.

### Surface configuration

Configure the surface with:

- `format`: use the first format returned by `surface.get_capabilities().formats`
- `usage`: `TextureUsages::RENDER_ATTACHMENT`
- `alpha_mode`: `CompositeAlphaMode::Auto`
- `width`, `height`: initial window inner size
- `present_mode`: `PresentMode::Fifo` (vsync)

Reconfigure on every `WindowEvent::Resized`. Store the current `SurfaceConfiguration`
in `Presenter` so the resize path does not need to re-query capabilities.

### Render pipeline

Use a minimal pipeline with no vertex buffer. The vertex shader generates a
fullscreen quad from `vertex_index` (0..6 covering two triangles). The fragment
shader samples from a 2D RGBA texture using a nearest-neighbor sampler.

The WGSL shaders are embedded as `include_str!` literals in `openjill-render/src/`.
Keep them short; the vertex shader is under 20 lines, the fragment shader under
10 lines.

Bind group layout:
- Binding 0: `texture_2d<f32>` (the expanded RGBA frame)
- Binding 1: `sampler` (nearest-neighbor, clamp-to-edge)

Create the pipeline once at `Presenter::new`; do not recreate it on resize.

### Aspect-ratio scaling

The game's native resolution is 320x200. Scale the quad to fill the window while
preserving the aspect ratio (letterbox or pillarbox with black bars). Calculate
the quad's NDC extents in `Presenter::resize` and pass them to the vertex shader
as a push constant or uniform. Prefer a uniform buffer over push constants for
portability across wgpu backends.

## winit event handling

### Event loop structure

Use winit 0.30's `ApplicationHandler` trait. `GameApp` implements
`ApplicationHandler<()>` and is passed to `event_loop.run_app(&mut app)`.

Key handlers:

| Event | Action |
|---|---|
| `resumed` | Create window, build `Presenter`, load initial palette |
| `window_event(Resized)` | Call `presenter.resize(w, h)` |
| `window_event(CloseRequested)` | Call `event_loop.exit()` |
| `window_event(KeyboardInput)` | Translate to `InputCommand`, push to `pending_commands` |
| `about_to_wait` | Run one tick, call `presenter.present(palette)` |

`about_to_wait` is the main game update slot. In this phase it runs the
55 ms tick target via elapsed-time accumulation. A monotonic instant stored
in `GameApp` tracks when the last tick fired; if fewer than 55 ms have elapsed,
skip the tick but still present.

### Frame rate and tick separation

- Tick rate: 18.18 ticks/s (55 ms interval), matching the DOS original.
- Present rate: vsync (`PresentMode::Fifo`). Present every `about_to_wait`
  event, but advance game state only on accumulated tick intervals.
- Do not implement frame interpolation in this epic.

## Framebuffer ownership

`Presenter` owns:

- `framebuffer: [u8; 320 * 200]` - the indexed pixel buffer, updated by `clear`
  and `blit` calls.
- `rgba_buffer: [u8; 320 * 200 * 4]` - the expanded RGBA buffer, rebuilt on
  each `present` call by iterating the indexed buffer through the palette.
- `frame_texture: wgpu::Texture` - a `320x200` RGBA8Unorm texture on the GPU.
  Recreated only if the game resolution changes (it will not change in this
  epic).

The indexed framebuffer is a value owned entirely by `Presenter`. Callers write
into it only through `clear`, `blit`, and `draw_text`. No exterior code borrows
or holds a slice into it.

At `present` time:

1. Expand the indexed buffer into `rgba_buffer` using the provided palette.
2. Write `rgba_buffer` into `frame_texture` via `queue.write_texture`.
3. Encode a render pass targeting the current swap chain texture.
4. Submit the encoder and call `surface_texture.present()`.

## Palette conversion

### SHA color-map structure

Each `ShaTileSet` carries an optional `color_map: Vec<ShaColorMapEntry>`. Each
entry contains three 6-bit values (R, G, B in the VGA 0-63 range).

The VGA 6-bit-to-8-bit expansion formula is:

```
component_8bit = (component_6bit * 255 + 31) / 63
```

or equivalently `(component_6bit << 2) | (component_6bit >> 4)` for a
bit-replication approach. Either is acceptable; document the chosen formula in
the `Palette::from_sha_color_map` doc comment.

Index 0 is the transparent color (black by convention in VGA). When blitting
non-opaque tiles, skip pixels with index 0. When presenting the background frame
the transparent index is rendered as the palette entry for index 0.

### Palette source for this epic

Load the color map from the first SHA tileset that carries a non-empty color
map, or fall back to a synthetic 256-entry greyscale ramp if no color map is
present. Log which source was used. In later epics the correct palette will be
selected per screen/level; that selection logic belongs in `openjill-game`, not
in `openjill-render`.

### No palette code in `openjill-data`

Keep `ShaColorMapEntry` as a plain data struct in `openjill-data` (it already
is). The `Palette` type and conversion logic live in `openjill-core`. The
`Presenter` in `openjill-render` receives a `&Palette` and applies it; it does
not store palette-selection logic.

## Sprite/tile blitting

### Indexed pixel layout

SHA tiles store row-major indexed pixels. `width` and `height` come from the
image record. Each pixel is a `u8` index into the active palette. The `type`
field in the image record indicates encoding:

- `0`: uncompressed row-major bytes.
- Other values: investigate via the `sha-file-extractor` Java module before
  implementing; do not assume uncompressed. Add a `TileDecodeError::UnknownType`
  variant if an unsupported type is encountered rather than silently producing
  garbage pixels.

### Blit operation

`Presenter::blit(src, src_width, src_height, dst_x, dst_y, opaque)`:

- Clip `src` against the 320x200 framebuffer bounds.
- Copy each non-clipped pixel from `src` to `framebuffer[y * 320 + x]`.
- If `!opaque`, skip pixels where `src[i] == 0`.
- `dst_x` or `dst_y` may be negative (partial blit from left/top edge is valid).

No rotation or scaling is applied in this operation. Sprites and tiles are
always blitted at 1:1 pixel-to-framebuffer-cell ratio.

### Text drawing

SHA tilesets with the `is_font` flag set store ASCII-ordered glyph tiles. The
glyph for character `c` is at tile index `c - 32` (printable ASCII starts at
space, decimal 32).

`ShaFontTiles` wraps a `Vec<(Vec<u8>, u8, u8)>` (pixel data, width, height) per
glyph. `Presenter::draw_text` iterates characters, looks up glyph tiles, and
calls `blit` for each character at the appropriate x offset. Tab and newline are
not required in this epic; replace unsupported characters with the space glyph.

## Input mapping

### InputCommand placement

`InputCommand` lives in `openjill-core`. It is a pure enum with no window,
event, or GPU imports. `openjill-render` does not depend on `openjill-core`; the
translation from winit `KeyCode` to `InputCommand` happens in `openjill-game`.

### Default keyboard mapping

Based on `controls.properties` from the Java source:

| Key | InputCommand |
|---|---|
| Arrow Left | MoveLeft |
| Arrow Right | MoveRight |
| Arrow Up / Space / Alt | Jump |
| Arrow Down | Duck |
| Ctrl | ThrowItem |
| Tab | NextInventory |
| Backspace | PrevInventory |
| Escape | Pause / menu |
| Q (or window close) | Quit |

Store the mapping as a static array of `(KeyCode, InputCommand)` pairs in
`openjill-game`. Do not hardcode it in the winit event handler body.

### Key repeat

winit delivers both press and release events. For held-key actions (movement,
duck), track which `InputCommand` values are currently active by maintaining a
`BTreeSet<InputCommand>` (or bitfield) in `GameApp`. Pass the active set each
tick rather than individual events. `InputCommand` must implement `Ord` if a
`BTreeSet` is used; derive it.

### No gameplay logic in the renderer or input layer

`openjill-render` must not contain any game-rule code. `openjill-game` may
translate events and maintain the active-input set, but the rules about what
happens when Jill moves are entirely in `openjill-core`.

## Tests and acceptance checks

Use synthetic data for all committed tests. Do not commit original game bytes,
PNG atlases from real data, or screenshots from real data.

Required tests by child issue:

- `#38 winit/wgpu foundation`: event loop starts without panicking on headless
  CI (skip or mock if no display; use `DISPLAY` / `WAYLAND_DISPLAY` environment
  check), `Presenter::new` constructs without error on systems with a GPU,
  `resize` does not panic when called with zero dimensions.
- `#39 framebuffer and present`: `clear` fills the indexed buffer, `blit` copies
  expected pixel values into the framebuffer at correct offsets, `blit` clips
  correctly at framebuffer edges, negative `dst_x`/`dst_y` produce correct
  partial blit, transparent pixels are skipped when `opaque = false`, RGBA
  expansion for a synthetic palette produces correct byte values.
- `#40 palette expansion`: `Palette::from_sha_color_map` expands 6-bit to 8-bit
  correctly for boundary values (0 and 63), `rgba` returns opaque alpha, index-0
  expansion is consistent with the transparent convention.
- `#41 sprite/tile blitting and text`: blit a synthetic two-color tile and verify
  framebuffer contents, draw a single-character string and verify the glyph tile
  was blitted at the correct position, unknown tile type returns error rather
  than producing output.
- `#42 input command mapping`: key press produces the correct `InputCommand`,
  key release removes it from the active set, unmapped keys produce no command.
- `#43 integration`: manual check only (gated by `OPENJILL_DATA_DIR`). Open a
  window, load `JILL1.SHA`, blit tileset index 0 tile 0 at (0, 0), expand
  through the SHA color map, present, and assert no GPU errors. This check is
  skipped in CI unless `OPENJILL_DATA_DIR` is set and a display is available.

Run `cargo test -p openjill-render` and `cargo test -p openjill-core` during
each child issue. Run the full workspace suite (`cargo test --workspace`) and the
Taskfile lint checks before marking the epic complete.

### Headless CI strategy

wgpu can use its `vulkan-portability` or software Vulkan (lavapipe) backend in
CI. Add a `--features test-headless` feature to `openjill-render` that
substitutes a null presenter for environments without a GPU or display. Tests
that require real GPU behavior should be marked `#[ignore]` unless
`OPENJILL_DATA_DIR` and `DISPLAY`/`WAYLAND_DISPLAY` are set.

## Known risks and handoff notes

- **winit 0.30 API**: `ApplicationHandler` is the correct trait for winit 0.30.
  Confirm the exact minor version in workspace dependencies before starting
  issue #38; the API stabilized in 0.30.0 and has been stable since.
- **wgpu backend portability**: wgpu 24 supports Vulkan, Metal, DX12, and WebGPU.
  The nearest-neighbor sampler (`FilterMode::Nearest`) is supported on all
  backends. Test on at least one non-Vulkan backend (Metal on macOS or DX12 on
  Windows) before marking the epic done.
- **Tile `type` field**: the SHA parser preserves the `type` byte but the Rust
  code does not yet decode non-zero types. Issue #41 must identify which `type`
  values appear in `JILL1.SHA` and implement only those required for episode 1
  rendering. Do not implement speculative tile types.
- **Font tileset identity**: the SHA `is_font` flag is the intended font
  discriminator, but the actual tileset index used for in-game text must be
  confirmed against `openjill-core` `TileManager` usage. Log the selected font
  tileset index during startup for verification.
- **Palette per screen**: this epic loads a single palette from the first
  available SHA color map. The Core Runtime epic (5) will select the correct
  palette per screen/level. Issue #43 documents this limitation; do not attempt
  to solve it here.
- **55 ms timing drift**: elapsed-time accumulation will drift under vsync if the
  display runs at non-18 Hz multiples. A fixed-tick accumulator with carry-over
  is sufficient for this epic. Exact DOS timing parity is deferred.
- **Unsafe code**: `forbid(unsafe_code)` is currently set in `openjill-render`.
  `bytemuck` provides safe casting for the RGBA buffer, so this restriction can
  be maintained. Do not remove `forbid(unsafe_code)` to work around a
  `bytemuck` usage issue; fix the usage instead.
- **Handoff to Core Runtime agent (epic 5)**: the renderer exposes `clear`,
  `blit`, `draw_text`, and `present`. The core runtime agent will call these
  from screen handlers. The `RenderCommand` abstraction belongs in that epic, not
  this one.
- **Implement child issues in order**: `#38`, `#39`, `#40`, `#41`, `#42`,
  `#43`.
- Add doc comments for every new module, type, field, function, and method per
  `AGENTS.md`.
