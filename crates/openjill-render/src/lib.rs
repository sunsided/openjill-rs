#![forbid(unsafe_code)]

use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
pub use openjill_core::Palette;
use openjill_core::{ClipRect, FontSize, RenderCommand};
use openjill_data::sha::{ShaFile, ShaTile, ShaTileSet};
use thiserror::Error;
use wgpu::{
    AddressMode, Backends, BindGroup, Buffer, BufferBindingType, BufferUsages, ColorTargetState,
    ColorWrites, CompositeAlphaMode, CurrentSurfaceTexture, Device, DeviceDescriptor, Extent3d,
    Features, FilterMode, FragmentState, Instance, InstanceDescriptor, Limits, LoadOp,
    MultisampleState, Operations, Origin3d, PipelineCompilationOptions, PipelineLayoutDescriptor,
    PowerPreference, PresentMode, PrimitiveState, Queue, RenderPassColorAttachment,
    RenderPassDescriptor, RenderPipeline, RequestAdapterError, RequestAdapterOptions,
    SamplerBindingType, ShaderModuleDescriptor, ShaderSource, ShaderStages, StoreOp, Surface,
    SurfaceConfiguration, SurfaceTexture, TexelCopyBufferLayout, TexelCopyTextureInfo, Texture,
    TextureAspect, TextureDescriptor, TextureDimension, TextureFormat, TextureSampleType,
    TextureUsages, TextureViewDescriptor, TextureViewDimension, VertexState,
};
use winit::window::Window;

/// Width of the indexed framebuffer in pixels.
pub const FRAMEBUFFER_WIDTH: usize = 320;
/// Height of the indexed framebuffer in pixels.
pub const FRAMEBUFFER_HEIGHT: usize = 200;
/// Total number of pixels in the indexed framebuffer.
pub const FRAMEBUFFER_PIXELS: usize = FRAMEBUFFER_WIDTH * FRAMEBUFFER_HEIGHT;
/// Total number of bytes in the expanded RGBA framebuffer.
const RGBA_BUFFER_BYTES: usize = FRAMEBUFFER_PIXELS * 4;
/// Native game aspect ratio used for letterbox and pillarbox scaling.
const GAME_ASPECT_RATIO: f32 = FRAMEBUFFER_WIDTH as f32 / FRAMEBUFFER_HEIGHT as f32;

/// Pixel value emitted by [`DecodedGlyphTile::from_sha_tile`] for transparent
/// background pixels.
///
/// SHA font tilesets store glyph pixels at the tileset's `bit_depth` (typically
/// 2 bits per pixel for Jill's `JILL1.SHA` fonts), with the maximum-value
/// pixel acting as the transparent background and the lower values (`1`, `2`
/// for `bit_depth == 2`) acting as solid / anti-aliased foreground.  To keep
/// the [`draw_text_indexed`] / [`draw_text_colorized_indexed`] hot loops
/// simple, the decoder normalises every glyph's background pixels to this
/// sentinel.  Any non-sentinel pixel writes the requested color index.
const FONT_GLYPH_TRANSPARENT_PIXEL: u8 = 0;

/// Palette index offset added to the caller's logical EGA color when
/// rendering [`RenderCommand::DrawText`].
///
/// Mirrors `TextManager.BACK_COLOR_SHIFT = 8` in the Java reference, which
/// promotes the dark EGA indices `0..=7` to their bright EGA counterparts
/// `8..=15` for all text rendering paths
/// (`drawSmallText` / `drawBigText` / `grapSmallLetter`).
const TEXT_COLOR_BRIGHT_SHIFT: u8 = 8;

/// Highest palette index reachable through the bright-EGA text shift,
/// matching `TextManager.BACKGROUND_COLOR_WHITE` in the Java reference
/// (`COLOR_WHITE + BACK_COLOR_SHIFT = 15`).
const BRIGHT_EGA_MAX: u8 = 15;

/// Decoded SHA font glyph tiles used by [`Presenter::draw_text`].
///
/// Construct this wrapper with [`ShaFontTiles::from_tileset`] after loading a
/// font tileset from `JILL1.SHA`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShaFontTiles {
    /// Glyph tiles indexed by raw ASCII codepoint (`index = c as u8`).
    ///
    /// `JILL1.SHA` font tilesets carry 128 entries whose glyph at index `c`
    /// corresponds to ASCII codepoint `c`.  Empirically tile `65` renders the
    /// letter `A`, tile `33` renders `!`, and so on — the layout matches the
    /// raw codepoint rather than the printable-ASCII offset `c - 32` used by
    /// some other DOS fonts.
    glyphs: Vec<DecodedGlyphTile>,
}

impl ShaFontTiles {
    /// Decodes a SHA font tileset into blit-ready glyph pixels.
    pub fn from_tileset(tileset: &ShaTileSet) -> Result<Self, TileDecodeError> {
        if !tileset.is_font() {
            return Err(TileDecodeError::NotFontTileset {
                tileset_index: tileset.entry_index(),
            });
        }
        let bit_depth = tileset.bit_depth();
        let mut glyphs = Vec::with_capacity(tileset.tiles().len());
        for tile in tileset.tiles() {
            glyphs.push(DecodedGlyphTile::from_sha_tile(tile, bit_depth)?);
        }
        Ok(Self { glyphs })
    }

    /// Returns the glyph tile for `character`, replacing characters that
    /// are not printable ASCII or space with the space glyph at the
    /// codepoint position `0x20`.
    ///
    /// Printable ASCII (`0x21..=0x7E`) and space (`0x20`) look up their
    /// raw codepoint; everything else (control codes, non-ASCII) falls
    /// back to space rather than rendering a stray printable glyph at the
    /// truncated `c as u8` position.  ASCII codepoints `0x01..=0x08` are
    /// explicitly allowed through because the bundled font tilesets carry
    /// the eight-frame menu cursor animation at those positions and
    /// callers (the start menu cursor) need to draw it.
    fn glyph_for_character(&self, character: char) -> Option<&DecodedGlyphTile> {
        let glyph_index = if character.is_ascii_graphic()
            || character == ' '
            || matches!(character as u32, 0x01..=0x08)
        {
            usize::from(character as u8)
        } else {
            usize::from(b' ')
        };
        self.glyphs
            .get(glyph_index)
            .or_else(|| self.glyphs.get(usize::from(b' ')))
            .or_else(|| self.glyphs.first())
    }
}

/// One decoded glyph tile containing row-major indexed source pixels.
///
/// Each value represents one ASCII glyph image from a SHA font tileset and
/// stores the exact pixels plus dimensions consumed by the blitter.
#[derive(Clone, Debug, Eq, PartialEq)]
struct DecodedGlyphTile {
    /// Row-major indexed source pixels for this glyph.
    pixels: Vec<u8>,
    /// Glyph width in pixels.
    width: u8,
    /// Glyph height in pixels.
    height: u8,
}

impl DecodedGlyphTile {
    /// Decodes one SHA tile record into a glyph tile, normalising the
    /// tileset's transparent-pixel value to [`FONT_GLYPH_TRANSPARENT_PIXEL`].
    ///
    /// SHA font tilesets store every pixel as a 1-byte value in the range
    /// `0 .. (1 << bit_depth)`, with the largest value (e.g. `3` for
    /// `bit_depth == 2`) reserved for the transparent background and the
    /// lower values acting as foreground / anti-aliased intensities.  The
    /// renderer's blit and colorise-and-blit paths use a single sentinel
    /// pixel value to drop transparent pixels, so this constructor remaps
    /// every `transparent_value` it sees to [`FONT_GLYPH_TRANSPARENT_PIXEL`]
    /// up-front.  Glyphs whose tileset reports `bit_depth >= 8` retain their
    /// raw pixel values because no per-tileset transparent sentinel exists
    /// at that depth.
    fn from_sha_tile(tile: &ShaTile, bit_depth: u8) -> Result<Self, TileDecodeError> {
        if tile.data_format() != 0 {
            return Err(TileDecodeError::UnknownType {
                tileset_index: tile.tileset_index(),
                tile_index: tile.image_index(),
                data_format: tile.data_format(),
            });
        }
        let pixels = if bit_depth < 8 {
            let transparent = (1_u8 << bit_depth).wrapping_sub(1);
            tile.indexed_pixels()
                .iter()
                .map(|&pixel| {
                    if pixel == transparent {
                        FONT_GLYPH_TRANSPARENT_PIXEL
                    } else {
                        pixel
                    }
                })
                .collect()
        } else {
            tile.indexed_pixels().to_vec()
        };
        Ok(Self {
            pixels,
            width: tile.width(),
            height: tile.height(),
        })
    }
}

/// Errors returned while decoding SHA tiles into renderer-ready pixel buffers.
#[derive(Debug, Error, Eq, PartialEq)]
pub enum TileDecodeError {
    /// The SHA tile `type`/`data_format` is unsupported by the current decoder.
    #[error("unsupported SHA tile type {data_format} in tileset {tileset_index} tile {tile_index}")]
    UnknownType {
        /// Header entry index of the parent tileset.
        tileset_index: usize,
        /// Tile index inside the parent tileset.
        tile_index: usize,
        /// Unsupported SHA tile `type`/`data_format` value.
        data_format: u8,
    },
    /// The SHA tileset passed to a font decoder is not flagged as a font tileset.
    #[error("SHA tileset {tileset_index} is not a font tileset")]
    NotFontTileset {
        /// Header entry index of the offending tileset.
        tileset_index: usize,
    },
}

/// Errors returned when preparing indexed framebuffer data for presentation.
#[derive(Debug, Error, Eq, PartialEq)]
pub enum FramebufferError {
    /// The indexed framebuffer length did not match the fixed 320×200 contract.
    #[error("indexed framebuffer must contain {expected} bytes, got {actual}")]
    InvalidLength {
        /// The required framebuffer byte length.
        expected: usize,
        /// The actual slice length provided by the caller.
        actual: usize,
    },
}

/// Reusable WGPU resources for presenting OpenJill's indexed 320×200 framebuffer.
pub struct IndexedFramePainter {
    /// Expanded RGBA buffer uploaded to the GPU each frame.
    rgba_buffer: Box<[u8]>,
    /// GPU texture that stores the expanded RGBA frame.
    frame_texture: Texture,
    /// Bind-group containing the frame texture, sampler, and scaling uniform.
    frame_bind_group: BindGroup,
    /// Render pipeline drawing a scaled full-screen quad.
    present_pipeline: RenderPipeline,
    /// Uniform buffer storing aspect-ratio scaling factors for the vertex shader.
    scale_uniform_buffer: Buffer,
}

impl IndexedFramePainter {
    /// Creates a new indexed-frame painter for a target format and viewport size.
    pub fn new(
        device: &Device,
        queue: &Queue,
        target_format: TextureFormat,
        width: u32,
        height: u32,
    ) -> Self {
        let (width, height) = clamp_surface_size(width, height);
        let frame_texture = device.create_texture(&TextureDescriptor {
            label: Some("openjill-render-frame-texture"),
            size: Extent3d {
                width: FRAMEBUFFER_WIDTH as u32,
                height: FRAMEBUFFER_HEIGHT as u32,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8UnormSrgb,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let frame_texture_view = frame_texture.create_view(&TextureViewDescriptor::default());
        let frame_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("openjill-render-frame-sampler"),
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            address_mode_w: AddressMode::ClampToEdge,
            mag_filter: FilterMode::Nearest,
            min_filter: FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..wgpu::SamplerDescriptor::default()
        });

        let initial_scale = ScaleUniform {
            scale: calculate_present_scale(width, height),
            _padding: [0.0, 0.0],
        };
        let scale_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("openjill-render-scale-uniform-buffer"),
            size: std::mem::size_of::<ScaleUniform>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&scale_uniform_buffer, 0, bytemuck::bytes_of(&initial_scale));

        let frame_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("openjill-render-frame-bind-group-layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: TextureSampleType::Float { filterable: true },
                            view_dimension: TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(SamplerBindingType::Filtering),
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: ShaderStages::VERTEX,
                        ty: wgpu::BindingType::Buffer {
                            ty: BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });
        let frame_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("openjill-render-frame-bind-group"),
            layout: &frame_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&frame_texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&frame_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: scale_uniform_buffer.as_entire_binding(),
                },
            ],
        });

        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("openjill-render-present-shader"),
            source: ShaderSource::Wgsl(include_str!("present.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("openjill-render-present-pipeline-layout"),
            bind_group_layouts: &[Some(&frame_bind_group_layout)],
            immediate_size: 0,
        });
        let present_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("openjill-render-present-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: PipelineCompilationOptions::default(),
            },
            fragment: Some(FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(ColorTargetState {
                    format: target_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: ColorWrites::ALL,
                })],
                compilation_options: PipelineCompilationOptions::default(),
            }),
            primitive: PrimitiveState::default(),
            depth_stencil: None,
            multisample: MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        Self {
            rgba_buffer: vec![0_u8; RGBA_BUFFER_BYTES].into_boxed_slice(),
            frame_texture,
            frame_bind_group,
            present_pipeline,
            scale_uniform_buffer,
        }
    }

    /// Updates the viewport scaling factors for a new target size.
    pub fn resize(&mut self, queue: &Queue, width: u32, height: u32) {
        let (width, height) = clamp_surface_size(width, height);
        let uniform = ScaleUniform {
            scale: calculate_present_scale(width, height),
            _padding: [0.0, 0.0],
        };
        queue.write_buffer(&self.scale_uniform_buffer, 0, bytemuck::bytes_of(&uniform));
    }

    /// Expands one indexed framebuffer and uploads it to the painter texture.
    pub fn prepare(
        &mut self,
        queue: &Queue,
        framebuffer: &[u8],
        palette: &Palette,
    ) -> Result<(), FramebufferError> {
        if framebuffer.len() != FRAMEBUFFER_PIXELS {
            return Err(FramebufferError::InvalidLength {
                expected: FRAMEBUFFER_PIXELS,
                actual: framebuffer.len(),
            });
        }
        expand_indexed_framebuffer(framebuffer, &mut self.rgba_buffer, palette);
        queue.write_texture(
            TexelCopyTextureInfo {
                texture: &self.frame_texture,
                mip_level: 0,
                origin: Origin3d::ZERO,
                aspect: TextureAspect::All,
            },
            &self.rgba_buffer,
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some((FRAMEBUFFER_WIDTH * 4) as u32),
                rows_per_image: Some(FRAMEBUFFER_HEIGHT as u32),
            },
            Extent3d {
                width: FRAMEBUFFER_WIDTH as u32,
                height: FRAMEBUFFER_HEIGHT as u32,
                depth_or_array_layers: 1,
            },
        );
        Ok(())
    }

    /// Draws the prepared framebuffer into the active render pass.
    pub fn paint(&self, render_pass: &mut wgpu::RenderPass<'_>) {
        render_pass.set_pipeline(&self.present_pipeline);
        render_pass.set_bind_group(0, &self.frame_bind_group, &[]);
        render_pass.draw(0..6, 0..1);
    }
}

/// Owns the wgpu instance, surface, and presentation state for the active window.
pub struct Presenter {
    /// WGPU instance that created this presenter surface.
    _instance: Instance,
    /// Surface bound to the active native window.
    surface: Surface<'static>,
    /// Logical GPU device used to encode and submit rendering commands.
    device: Device,
    /// GPU queue used to submit encoded command buffers.
    queue: Queue,
    /// Active swap-chain configuration used for presentation.
    surface_config: SurfaceConfiguration,
    /// Indexed 320x200 framebuffer updated by clear and blit operations.
    framebuffer: Box<[u8]>,
    /// Reusable indexed-frame GPU resources shared by window and egui presentation paths.
    frame_painter: IndexedFramePainter,
}

/// Packed uniform values sent to the vertex shader for aspect-ratio scaling.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ScaleUniform {
    /// Horizontal and vertical scale factors in NDC space.
    scale: [f32; 2],
    /// Padding to satisfy 16-byte alignment requirements.
    _padding: [f32; 2],
}

impl Presenter {
    /// Creates a presenter by resolving instance, surface, adapter, device, and queue.
    pub async fn new(window: Arc<Window>) -> Result<Self, PresenterError> {
        let instance = Instance::new(InstanceDescriptor {
            backends: Backends::all(),
            ..InstanceDescriptor::new_without_display_handle()
        });
        let window_size = clamp_surface_size(window.inner_size().width, window.inner_size().height);
        let surface = instance.create_surface(window)?;
        let adapter = instance
            .request_adapter(&RequestAdapterOptions {
                power_preference: PowerPreference::None,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await?;
        let (device, queue) = adapter
            .request_device(&DeviceDescriptor {
                label: Some("openjill-render-device"),
                required_features: Features::empty(),
                required_limits: Limits::default(),
                memory_hints: wgpu::MemoryHints::Performance,
                experimental_features: wgpu::ExperimentalFeatures::default(),
                trace: wgpu::Trace::Off,
            })
            .await?;
        let capabilities = surface.get_capabilities(&adapter);
        let format = select_surface_format(&capabilities.formats)?;
        let surface_config = SurfaceConfiguration {
            usage: TextureUsages::RENDER_ATTACHMENT,
            format,
            width: window_size.0,
            height: window_size.1,
            present_mode: PresentMode::Fifo,
            alpha_mode: CompositeAlphaMode::Auto,
            view_formats: Vec::new(),
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &surface_config);
        let frame_painter = IndexedFramePainter::new(
            &device,
            &queue,
            surface_config.format,
            surface_config.width,
            surface_config.height,
        );

        Ok(Self {
            _instance: instance,
            surface,
            device,
            queue,
            surface_config,
            framebuffer: vec![0_u8; FRAMEBUFFER_PIXELS].into_boxed_slice(),
            frame_painter,
        })
    }

    /// Reconfigures the presenter surface for a new window size.
    pub fn resize(&mut self, width: u32, height: u32) {
        let (width, height) = clamp_surface_size(width, height);
        if self.surface_config.width == width && self.surface_config.height == height {
            return;
        }
        self.surface_config.width = width;
        self.surface_config.height = height;
        self.surface.configure(&self.device, &self.surface_config);
        self.frame_painter.resize(&self.queue, width, height);
    }

    /// Reconfigures the surface with the current configuration, used for `Lost`/`Outdated` recovery.
    pub fn reconfigure(&mut self) {
        self.surface.configure(&self.device, &self.surface_config);
    }

    /// Clears the indexed framebuffer to one palette index.
    pub fn clear(&mut self, color_index: u8) {
        self.framebuffer.fill(color_index);
    }

    /// Blits a row-major indexed image into the framebuffer with clipping and optional transparency.
    pub fn blit(
        &mut self,
        src: &[u8],
        src_width: u16,
        src_height: u16,
        dst_x: i32,
        dst_y: i32,
        opaque: bool,
    ) {
        blit_indexed(
            &mut self.framebuffer,
            src,
            src_width,
            src_height,
            dst_x,
            dst_y,
            opaque,
            None,
        );
    }

    /// Draws one text string by blitting SHA font glyph tiles left-to-right.
    ///
    /// The first glyph is rendered at `(x, y)` using top-left framebuffer
    /// coordinates; each subsequent glyph advances by the previous glyph width.
    /// Printable ASCII characters (`' '` through `'~'`) map directly to SHA
    /// glyph index `c - 32`. Tabs, newlines, and out-of-range characters are
    /// replaced by the space glyph.
    pub fn draw_text(&mut self, text: &str, x: i32, y: i32, font: &ShaFontTiles) {
        draw_text_indexed(&mut self.framebuffer, text, x, y, font);
    }

    /// Expands the indexed framebuffer to RGBA, uploads it, and presents one frame.
    pub fn present(&mut self, palette: &Palette) -> Result<(), PresenterError> {
        self.frame_painter
            .prepare(&self.queue, &self.framebuffer, palette)?;
        let frame = acquire_surface_texture(self.surface.get_current_texture())?;
        let view = frame.texture.create_view(&TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("openjill-render-present-encoder"),
            });
        {
            let mut render_pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("openjill-render-present-pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: Operations {
                        load: LoadOp::Clear(wgpu::Color::BLACK),
                        store: StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            self.frame_painter.paint(&mut render_pass);
        }
        self.queue.submit(std::iter::once(encoder.finish()));
        frame.present();
        Ok(())
    }

    /// Executes render commands against the internal framebuffer, then presents one frame.
    ///
    /// Each command is dispatched in order: [`RenderCommand::Clear`] fills the entire buffer,
    /// [`RenderCommand::Blit`] resolves tile pixel data from `sha` by header entry index and blits
    /// it, [`RenderCommand::DrawText`] uses the first font tileset found in `sha` (skipped when
    /// none exists or decoding fails), and [`RenderCommand::FillRect`] fills a framebuffer region.
    /// After all commands are dispatched, [`Presenter::present`] uploads the result to the GPU.
    pub fn execute_and_present(
        &mut self,
        commands: &[RenderCommand],
        sha: &ShaFile,
        palette: &Palette,
    ) -> Result<(), PresenterError> {
        // Clear the framebuffer before each frame so pixels left over from
        // earlier frames (transparent map cells, dismissed overlays) cannot
        // bleed into the new frame.  Palette index 0 is the canonical
        // transparent black sentinel; status-bar and screen commands write
        // over it as needed.
        self.framebuffer.fill(0);
        execute_commands_on_framebuffer(&mut self.framebuffer, commands, sha);
        self.present(palette)
    }
}

/// Maps a [`CurrentSurfaceTexture`] result into a usable texture or a typed surface error.
fn acquire_surface_texture(current: CurrentSurfaceTexture) -> Result<SurfaceTexture, SurfaceError> {
    match current {
        CurrentSurfaceTexture::Success(texture) | CurrentSurfaceTexture::Suboptimal(texture) => {
            Ok(texture)
        }
        CurrentSurfaceTexture::Timeout => Err(SurfaceError::Timeout),
        CurrentSurfaceTexture::Outdated => Err(SurfaceError::Outdated),
        CurrentSurfaceTexture::Lost => Err(SurfaceError::Lost),
        CurrentSurfaceTexture::Occluded => Err(SurfaceError::Occluded),
        CurrentSurfaceTexture::Validation => Err(SurfaceError::Validation),
    }
}

/// Expands indexed pixels into RGBA bytes using a palette lookup per pixel.
///
/// `framebuffer` must be exactly [`FRAMEBUFFER_PIXELS`] bytes; `rgba_buffer` must be exactly
/// [`RGBA_BUFFER_BYTES`] bytes. The presenter owns these buffers, so the lengths are guaranteed
/// at the only call site.
fn expand_indexed_framebuffer(framebuffer: &[u8], rgba_buffer: &mut [u8], palette: &Palette) {
    debug_assert_eq!(framebuffer.len(), FRAMEBUFFER_PIXELS);
    debug_assert_eq!(rgba_buffer.len(), RGBA_BUFFER_BYTES);
    for (source, destination) in framebuffer.iter().zip(rgba_buffer.chunks_exact_mut(4)) {
        destination.copy_from_slice(&palette.rgba(*source));
    }
}

/// Blits indexed source pixels into a framebuffer with clipping and optional transparency.
///
/// Returns without writing any pixels when `src` is shorter than `src_width * src_height`,
/// so a too-small source never produces a partially-updated framebuffer.
#[allow(clippy::too_many_arguments)]
fn blit_indexed(
    framebuffer: &mut [u8],
    src: &[u8],
    src_width: u16,
    src_height: u16,
    dst_x: i32,
    dst_y: i32,
    opaque: bool,
    clip: Option<ClipRect>,
) {
    debug_assert_eq!(framebuffer.len(), FRAMEBUFFER_PIXELS);
    let src_width = src_width as usize;
    let src_height = src_height as usize;
    if src_width == 0 || src_height == 0 {
        return;
    }
    let Some(required_bytes) = src_width.checked_mul(src_height) else {
        return;
    };
    if src.len() < required_bytes {
        return;
    }

    for src_y in 0..src_height {
        let target_y = dst_y + src_y as i32;
        if !(0..FRAMEBUFFER_HEIGHT as i32).contains(&target_y) {
            continue;
        }

        for src_x in 0..src_width {
            let target_x = dst_x + src_x as i32;
            if !(0..FRAMEBUFFER_WIDTH as i32).contains(&target_x) {
                continue;
            }
            if let Some(rect) = clip
                && !rect.contains(target_x, target_y)
            {
                continue;
            }

            let src_index = src_y * src_width + src_x;
            let pixel = src[src_index];
            if !opaque && pixel == 0 {
                continue;
            }

            let framebuffer_index = target_y as usize * FRAMEBUFFER_WIDTH + target_x as usize;
            framebuffer[framebuffer_index] = pixel;
        }
    }
}

/// Draws text by blitting glyph tiles into a 320x200 indexed framebuffer.
///
/// `framebuffer` must contain exactly one byte per pixel in row-major
/// `y * 320 + x` order. `x`/`y` are top-left destination coordinates for the
/// first glyph, and each glyph advances the text cursor by its width. Character
/// lookup follows printable ASCII index `c - 32`, with unsupported characters
/// replaced by the space glyph. When the font has no glyphs, drawing is a no-op.
fn draw_text_indexed(framebuffer: &mut [u8], text: &str, x: i32, y: i32, font: &ShaFontTiles) {
    let mut cursor_x = x;
    for character in text.chars() {
        let Some(glyph) = font.glyph_for_character(character) else {
            continue;
        };
        blit_indexed(
            framebuffer,
            &glyph.pixels,
            u16::from(glyph.width),
            u16::from(glyph.height),
            cursor_x,
            y,
            false,
            None,
        );
        cursor_x += i32::from(glyph.width);
    }
}

/// Draws text as a glyph mask filled with one palette index.
///
/// Non-zero glyph pixels write `color_index`; zero glyph pixels remain transparent. Character
/// lookup and cursor advancement match [`draw_text_indexed`].
fn draw_text_colorized_indexed(
    framebuffer: &mut [u8],
    text: &str,
    x: i32,
    y: i32,
    font: &ShaFontTiles,
    color_index: u8,
) {
    let mut cursor_x = x;
    for character in text.chars() {
        let Some(glyph) = font.glyph_for_character(character) else {
            continue;
        };

        for glyph_y in 0..usize::from(glyph.height) {
            let target_y = y + glyph_y as i32;
            if !(0..FRAMEBUFFER_HEIGHT as i32).contains(&target_y) {
                continue;
            }

            for glyph_x in 0..usize::from(glyph.width) {
                let target_x = cursor_x + glyph_x as i32;
                if !(0..FRAMEBUFFER_WIDTH as i32).contains(&target_x) {
                    continue;
                }

                let glyph_index = glyph_y * usize::from(glyph.width) + glyph_x;
                if glyph.pixels[glyph_index] == 0 {
                    continue;
                }

                let framebuffer_index = target_y as usize * FRAMEBUFFER_WIDTH + target_x as usize;
                framebuffer[framebuffer_index] = color_index;
            }
        }

        cursor_x += i32::from(glyph.width);
    }
}

/// Fills a rectangle in a 320×200 indexed framebuffer with `color`, clipping to buffer bounds.
///
/// Returns immediately when `width` or `height` is zero. Negative or out-of-bounds coordinates
/// are clipped to the framebuffer edges so no out-of-bounds write is possible.
fn fill_rect_indexed(framebuffer: &mut [u8], x: i32, y: i32, width: u32, height: u32, color: u8) {
    debug_assert_eq!(framebuffer.len(), FRAMEBUFFER_PIXELS);
    if width == 0 || height == 0 {
        return;
    }
    let x_start = x.clamp(0, FRAMEBUFFER_WIDTH as i32) as usize;
    let x_end = (x as i64 + width as i64).clamp(0, FRAMEBUFFER_WIDTH as i64) as usize;
    let y_start = y.clamp(0, FRAMEBUFFER_HEIGHT as i32) as usize;
    let y_end = (y as i64 + height as i64).clamp(0, FRAMEBUFFER_HEIGHT as i64) as usize;
    if x_start >= x_end || y_start >= y_end {
        return;
    }
    for row in y_start..y_end {
        let row_start = row * FRAMEBUFFER_WIDTH + x_start;
        let row_end = row * FRAMEBUFFER_WIDTH + x_end;
        framebuffer[row_start..row_end].fill(color);
    }
}

/// Dispatches a slice of render commands onto a 320×200 indexed framebuffer.
///
/// Resolves [`RenderCommand::Blit`] tile data from `sha` by matching the
/// command's `tileset` value against each [`ShaTileSet::entry_index`].  Two
/// font tilesets are decoded once per call and selected per
/// [`RenderCommand::DrawText`] via the command's [`FontSize`] field:
/// [`FontSize::Small`] uses the second font tileset found in `sha` (the
/// 6x6 body-text font in the shipped `JILL1.SHA`), and [`FontSize::Big`]
/// uses the first font tileset (the 8x8 `bigtext` font).  When the
/// requested font is not present in `sha` the renderer falls back to
/// whichever font is available so partial fonts still render text; if
/// neither font is present the `DrawText` is skipped silently.
fn execute_commands_on_framebuffer(
    framebuffer: &mut [u8],
    commands: &[RenderCommand],
    sha: &ShaFile,
) {
    let mut font_tilesets = sha.tilesets().iter().filter(|ts| ts.is_font());
    let big_font = font_tilesets
        .next()
        .and_then(|ts| ShaFontTiles::from_tileset(ts).ok());
    let small_font = font_tilesets
        .next()
        .and_then(|ts| ShaFontTiles::from_tileset(ts).ok());

    for command in commands {
        match command {
            RenderCommand::Clear { color } => {
                framebuffer.fill(*color);
            }
            RenderCommand::Blit {
                tileset,
                tile,
                x,
                y,
                opaque,
                clip,
            } => {
                let tileset_idx = *tileset as usize;
                let tile_idx = *tile as usize;
                if let Some(ts) = sha
                    .tilesets()
                    .iter()
                    .find(|ts| ts.entry_index() == tileset_idx)
                    && let Some(t) = ts.tiles().get(tile_idx)
                {
                    blit_indexed(
                        framebuffer,
                        t.indexed_pixels(),
                        u16::from(t.width()),
                        u16::from(t.height()),
                        *x,
                        *y,
                        *opaque,
                        *clip,
                    );
                }
            }
            RenderCommand::DrawText {
                text,
                x,
                y,
                color_index,
                font,
            } => {
                let selected = match font {
                    FontSize::Small => small_font.as_ref().or(big_font.as_ref()),
                    FontSize::Big => big_font.as_ref().or(small_font.as_ref()),
                };
                if let Some(f) = selected {
                    // Promote the caller's logical EGA color to the matching
                    // bright-EGA index, matching `BACK_COLOR_SHIFT = 8` and
                    // the `BACKGROUND_COLOR_WHITE` clamp in
                    // `TextManagerImpl.initColorTextMap`.  Without this shift
                    // text renders at the dark half of the EGA palette and
                    // looks noticeably dimmer than the Java reference.
                    let bright = color_index
                        .saturating_add(TEXT_COLOR_BRIGHT_SHIFT)
                        .min(BRIGHT_EGA_MAX);
                    draw_text_colorized_indexed(framebuffer, text, *x, *y, f, bright);
                }
            }
            RenderCommand::FillRect {
                x,
                y,
                width,
                height,
                color,
            } => {
                fill_rect_indexed(framebuffer, *x, *y, *width, *height, *color);
            }
        }
    }
}

/// Computes letterbox/pillarbox scaling in NDC space for the current surface size.
fn calculate_present_scale(width: u32, height: u32) -> [f32; 2] {
    let width = width.max(1) as f32;
    let height = height.max(1) as f32;
    let window_aspect_ratio = width / height;

    if window_aspect_ratio > GAME_ASPECT_RATIO {
        [GAME_ASPECT_RATIO / window_aspect_ratio, 1.0]
    } else {
        [1.0, window_aspect_ratio / GAME_ASPECT_RATIO]
    }
}

/// Errors returned by the renderer presenter lifecycle.
#[derive(Debug, Error)]
pub enum PresenterError {
    /// Surface creation failed for the supplied native window.
    #[error("failed to create wgpu surface: {0}")]
    SurfaceCreation(#[from] wgpu::CreateSurfaceError),
    /// No compatible adapter could be selected for the current window surface.
    #[error("no compatible GPU adapter found for the active window surface: {0}")]
    NoCompatibleAdapter(#[from] RequestAdapterError),
    /// Device or queue creation failed after adapter selection.
    #[error("failed to create wgpu device: {0}")]
    RequestDevice(#[from] wgpu::RequestDeviceError),
    /// Surface capability enumeration returned no usable formats.
    #[error("wgpu surface reported no supported texture formats")]
    NoSurfaceFormats,
    /// Indexed framebuffer data was invalid for presentation.
    #[error(transparent)]
    Framebuffer(#[from] FramebufferError),
    /// Frame acquisition or presentation failed.
    #[error("surface presentation failed: {0}")]
    SurfaceError(#[from] SurfaceError),
}

/// Surface acquisition outcomes that callers can handle to recover or skip a frame.
#[derive(Debug, Error)]
pub enum SurfaceError {
    /// Frame acquisition timed out; callers should skip this frame and try again.
    #[error("timed out while acquiring the next surface texture")]
    Timeout,
    /// Surface configuration is outdated; callers should reconfigure and retry.
    #[error("surface configuration is outdated")]
    Outdated,
    /// Surface has been lost; callers should reconfigure and retry.
    #[error("surface has been lost and must be reconfigured")]
    Lost,
    /// Window is occluded; callers should skip this frame until visible again.
    #[error("window is occluded")]
    Occluded,
    /// Validation error reported during acquisition; the prior error scope captured it.
    #[error("validation error during surface acquisition")]
    Validation,
}

/// Clamps zero-sized window dimensions to the minimum valid surface extent.
fn clamp_surface_size(width: u32, height: u32) -> (u32, u32) {
    (width.max(1), height.max(1))
}

/// Selects the preferred presentable format, favoring sRGB where possible.
fn select_surface_format(formats: &[TextureFormat]) -> Result<TextureFormat, PresenterError> {
    const PREFERRED_FORMATS: [TextureFormat; 2] =
        [TextureFormat::Bgra8UnormSrgb, TextureFormat::Rgba8UnormSrgb];

    for preferred in PREFERRED_FORMATS {
        if formats.contains(&preferred) {
            return Ok(preferred);
        }
    }

    formats
        .first()
        .copied()
        .ok_or(PresenterError::NoSurfaceFormats)
}

/// Unit tests for presenter helper behavior.
#[cfg(test)]
mod tests {
    use super::{
        FRAMEBUFFER_HEIGHT, FRAMEBUFFER_PIXELS, FRAMEBUFFER_WIDTH, RGBA_BUFFER_BYTES, ShaFontTiles,
        TileDecodeError, blit_indexed, calculate_present_scale, clamp_surface_size,
        draw_text_colorized_indexed, draw_text_indexed, execute_commands_on_framebuffer,
        expand_indexed_framebuffer, fill_rect_indexed, select_surface_format,
    };
    use openjill_core::{FontSize, Palette, RenderCommand};
    use openjill_data::sha::ShaFile;
    use wgpu::TextureFormat;

    /// Verifies zero-sized dimensions are promoted to one pixel for surface safety.
    #[test]
    fn clamp_surface_size_promotes_zero_to_one() {
        assert_eq!(clamp_surface_size(0, 0), (1, 1));
        assert_eq!(clamp_surface_size(320, 200), (320, 200));
    }

    /// Prefers sRGB surface formats when they are available.
    #[test]
    fn select_surface_format_prefers_srgb() {
        let formats = [TextureFormat::Rgba8Unorm, TextureFormat::Bgra8UnormSrgb];
        assert_eq!(
            select_surface_format(&formats).expect("surface format should be selected"),
            TextureFormat::Bgra8UnormSrgb
        );
    }

    /// Keeps the full source frame when the window shares the native aspect ratio.
    #[test]
    fn calculate_present_scale_keeps_native_aspect_at_unity() {
        assert_eq!(calculate_present_scale(1600, 1000), [1.0, 1.0]);
    }

    /// Letterboxes frames for tall windows by shrinking only the Y axis.
    #[test]
    fn calculate_present_scale_letterboxes_tall_windows() {
        let [scale_x, scale_y] = calculate_present_scale(1200, 1200);
        assert!((scale_x - 1.0).abs() < 1e-6);
        assert!((scale_y - 0.625).abs() < 1e-6);
    }

    /// Pillarboxes frames for wide windows by shrinking only the X axis.
    #[test]
    fn calculate_present_scale_pillarboxes_wide_windows() {
        let [scale_x, scale_y] = calculate_present_scale(1920, 800);
        assert!((scale_x - 0.6666667).abs() < 1e-6);
        assert!((scale_y - 1.0).abs() < 1e-6);
    }

    /// Expands indexed pixels to RGBA bytes using the supplied palette lookup.
    #[test]
    fn expand_indexed_framebuffer_maps_palette_entries() {
        let mut framebuffer = [0_u8; FRAMEBUFFER_PIXELS];
        framebuffer[0] = 1;
        framebuffer[1] = 2;
        let mut entries = [[0_u8; 3]; 256];
        entries[1] = [10, 20, 30];
        entries[2] = [40, 50, 60];
        let palette = Palette::new(entries);
        let mut rgba_buffer = [0_u8; RGBA_BUFFER_BYTES];

        expand_indexed_framebuffer(&framebuffer, &mut rgba_buffer, &palette);

        assert_eq!(&rgba_buffer[0..4], &[10, 20, 30, 255]);
        assert_eq!(&rgba_buffer[4..8], &[40, 50, 60, 255]);
    }

    /// Verifies the greyscale fallback palette keeps index zero black for the transparent convention.
    #[test]
    fn default_palette_index_zero_is_black() {
        let palette = Palette::default();
        assert_eq!(palette.rgba(0), [0, 0, 0, 255]);
    }

    /// Ensures framebuffer geometry constants match the expected 320x200 resolution.
    #[test]
    fn framebuffer_geometry_constants_are_consistent() {
        assert_eq!(FRAMEBUFFER_WIDTH, 320);
        assert_eq!(FRAMEBUFFER_HEIGHT, 200);
        assert_eq!(FRAMEBUFFER_PIXELS, 64_000);
    }

    /// Copies source pixels into the framebuffer at the requested destination offset.
    #[test]
    fn blit_indexed_copies_expected_pixels() {
        let mut framebuffer = [0_u8; FRAMEBUFFER_PIXELS];
        let src = [1, 2, 3, 4];

        blit_indexed(&mut framebuffer, &src, 2, 2, 5, 7, true, None);

        assert_eq!(framebuffer[7 * FRAMEBUFFER_WIDTH + 5], 1);
        assert_eq!(framebuffer[7 * FRAMEBUFFER_WIDTH + 6], 2);
        assert_eq!(framebuffer[8 * FRAMEBUFFER_WIDTH + 5], 3);
        assert_eq!(framebuffer[8 * FRAMEBUFFER_WIDTH + 6], 4);
    }

    /// Clips source pixels that land outside framebuffer bounds, including negative destinations.
    #[test]
    fn blit_indexed_clips_negative_and_overflow_coordinates() {
        let mut framebuffer = [0_u8; FRAMEBUFFER_PIXELS];
        let src = [9, 8, 7, 6];

        blit_indexed(&mut framebuffer, &src, 2, 2, -1, -1, true, None);

        assert_eq!(framebuffer[0], 6);
        assert_eq!(framebuffer[1], 0);
        assert_eq!(framebuffer[FRAMEBUFFER_WIDTH], 0);
    }

    /// Skips transparent color index zero when opaque rendering is disabled.
    #[test]
    fn blit_indexed_skips_transparent_pixels_when_not_opaque() {
        let mut framebuffer = [5_u8; FRAMEBUFFER_PIXELS];
        let src = [0, 2, 3, 0];

        blit_indexed(&mut framebuffer, &src, 2, 2, 0, 0, false, None);

        assert_eq!(framebuffer[0], 5);
        assert_eq!(framebuffer[1], 2);
        assert_eq!(framebuffer[FRAMEBUFFER_WIDTH], 3);
        assert_eq!(framebuffer[FRAMEBUFFER_WIDTH + 1], 5);
    }

    /// Unit under test: `blit_indexed` honours an optional `ClipRect`.
    ///
    /// Preconditions: a 2x2 opaque source `[1, 2, 3, 4]` is blitted at
    /// destination `(4, 5)` with a clip rectangle that contains only the
    /// single pixel `(4, 5)`.
    ///
    /// Invariants asserted: only the top-left source pixel reaches the
    /// framebuffer; the other three are clipped because their destination
    /// coordinates fall outside the clip rectangle.
    #[test]
    fn blit_indexed_respects_clip_rectangle() {
        let mut framebuffer = [0_u8; FRAMEBUFFER_PIXELS];
        let src = [1_u8, 2, 3, 4];

        let clip = openjill_core::ClipRect {
            x: 4,
            y: 5,
            width: 1,
            height: 1,
        };
        blit_indexed(&mut framebuffer, &src, 2, 2, 4, 5, true, Some(clip));

        assert_eq!(framebuffer[5 * FRAMEBUFFER_WIDTH + 4], 1);
        assert_eq!(framebuffer[5 * FRAMEBUFFER_WIDTH + 5], 0);
        assert_eq!(framebuffer[6 * FRAMEBUFFER_WIDTH + 4], 0);
        assert_eq!(framebuffer[6 * FRAMEBUFFER_WIDTH + 5], 0);
    }

    /// Unit under test: `ShaFontTiles::from_tileset` decode-path validation.
    ///
    /// Preconditions: a synthetic SHA payload declares one font tile whose `data_format` is an
    /// unsupported non-zero type.
    ///
    /// Invariants asserted: decoding returns `TileDecodeError::UnknownType` and preserves the
    /// tileset index, tile index, and unsupported type value in the error payload.
    #[test]
    fn sha_font_tiles_reports_unknown_tile_type() {
        let sha = parse_synthetic_sha_with_font_tiles(&[(1, 1, 7, &[1])]);
        let error = ShaFontTiles::from_tileset(&sha.tilesets()[0])
            .expect_err("non-zero tile type should be rejected");
        assert_eq!(
            error,
            TileDecodeError::UnknownType {
                tileset_index: 0,
                tile_index: 0,
                data_format: 7,
            }
        );
    }

    /// Unit under test: `ShaFontTiles::from_tileset` font-flag precondition.
    ///
    /// Preconditions: a synthetic SHA payload declares one tileset whose `flags` field does not
    /// set `FLAG_FONT`.
    ///
    /// Invariants asserted: decoding rejects the tileset with
    /// `TileDecodeError::NotFontTileset`, preserving the tileset entry index.
    #[test]
    fn sha_font_tiles_rejects_non_font_tileset() {
        let sha = parse_synthetic_sha_tileset(&[(1, 1, 0, &[1])], 0);
        let error = ShaFontTiles::from_tileset(&sha.tilesets()[0])
            .expect_err("non-font tileset should be rejected");
        assert_eq!(error, TileDecodeError::NotFontTileset { tileset_index: 0 });
    }

    /// Unit under test: glyph placement in `draw_text_indexed`.
    ///
    /// Preconditions: a synthetic font carries 128 glyphs indexed by raw
    /// ASCII codepoint, with the `!` slot (codepoint 33) holding a 2x1
    /// glyph and every other slot a 1x1 transparent placeholder; the
    /// framebuffer starts at all zeroes.
    ///
    /// Invariants asserted: drawing `"!"` writes exactly that glyph's
    /// pixels at the requested X/Y origin, proving the character-to-glyph
    /// lookup uses the raw ASCII codepoint and the blit destination math
    /// is correct.
    #[test]
    fn draw_text_indexed_blits_single_character_at_expected_offset() {
        let mut tiles = ascii_indexed_glyph_table();
        tiles[usize::from(b'!')] = (2, 1, 0, vec![9_u8, 8]);
        let sha = parse_synthetic_sha_with_owned_font_tiles(&tiles);
        let font = ShaFontTiles::from_tileset(&sha.tilesets()[0])
            .expect("font tileset should decode successfully");
        let mut framebuffer = [0_u8; FRAMEBUFFER_PIXELS];

        draw_text_indexed(&mut framebuffer, "!", 10, 3, &font);

        assert_eq!(framebuffer[3 * FRAMEBUFFER_WIDTH + 10], 9);
        assert_eq!(framebuffer[3 * FRAMEBUFFER_WIDTH + 11], 8);
    }

    /// Unit under test: unsupported-character fallback in `draw_text_indexed`.
    ///
    /// Preconditions: a synthetic font carries 128 glyphs indexed by raw
    /// ASCII codepoint, with the space slot (codepoint 32) holding a single
    /// opaque pixel and every other slot a 1x1 transparent placeholder;
    /// text contains a newline plus an out-of-range non-ASCII character.
    ///
    /// Invariants asserted: each unsupported character is replaced with
    /// the space glyph, so both draws write the space glyph pixel value at
    /// the corresponding destination.
    #[test]
    fn draw_text_indexed_replaces_unsupported_characters_with_space_glyph() {
        let mut tiles = ascii_indexed_glyph_table();
        tiles[usize::from(b' ')] = (1, 1, 0, vec![7_u8]);
        let sha = parse_synthetic_sha_with_owned_font_tiles(&tiles);
        let font = ShaFontTiles::from_tileset(&sha.tilesets()[0])
            .expect("font tileset should decode successfully");
        let mut framebuffer = [0_u8; FRAMEBUFFER_PIXELS];

        draw_text_indexed(&mut framebuffer, "\né", 2, 4, &font);

        assert_eq!(framebuffer[4 * FRAMEBUFFER_WIDTH + 2], 7);
        assert_eq!(framebuffer[4 * FRAMEBUFFER_WIDTH + 3], 7);
    }

    /// Unit under test: colorized glyph mask rendering in
    /// `draw_text_colorized_indexed`.
    ///
    /// Preconditions: a synthetic font with the `!` slot carrying one
    /// non-zero pixel and one transparent pixel; the framebuffer starts at
    /// all zeroes.
    ///
    /// Invariants asserted: non-zero glyph pixels are written with the
    /// requested color index, while transparent glyph pixels remain
    /// untouched.
    #[test]
    fn draw_text_colorized_indexed_applies_requested_color_index() {
        let mut tiles = ascii_indexed_glyph_table();
        tiles[usize::from(b'!')] = (2, 1, 0, vec![9_u8, 0]);
        let sha = parse_synthetic_sha_with_owned_font_tiles(&tiles);
        let font = ShaFontTiles::from_tileset(&sha.tilesets()[0])
            .expect("font tileset should decode successfully");
        let mut framebuffer = [0_u8; FRAMEBUFFER_PIXELS];

        draw_text_colorized_indexed(&mut framebuffer, "!", 10, 3, &font, 12);

        assert_eq!(framebuffer[3 * FRAMEBUFFER_WIDTH + 10], 12);
        assert_eq!(framebuffer[3 * FRAMEBUFFER_WIDTH + 11], 0);
    }

    /// Builds a 128-entry glyph table where every codepoint defaults to a
    /// 1x1 transparent tile so callers can overwrite individual ASCII
    /// codepoints without filling the unused slots.
    fn ascii_indexed_glyph_table() -> Vec<(u8, u8, u8, Vec<u8>)> {
        // Width=1, height=1, data_format=0, pixel value 0 (transparent).
        vec![(1_u8, 1_u8, 0_u8, vec![0_u8]); 128]
    }

    /// Builds and parses a synthetic SHA payload that carries one font
    /// tileset whose tile records are owned by the caller (allows variable
    /// per-entry pixel lengths).
    fn parse_synthetic_sha_with_owned_font_tiles(tile_specs: &[(u8, u8, u8, Vec<u8>)]) -> ShaFile {
        let borrowed: Vec<(u8, u8, u8, &[u8])> = tile_specs
            .iter()
            .map(|(w, h, fmt, pixels)| (*w, *h, *fmt, pixels.as_slice()))
            .collect();
        parse_synthetic_sha_with_font_tiles(&borrowed)
    }

    /// Builds and parses a minimal SHA payload containing one font tileset with caller-provided
    /// tile records.
    fn parse_synthetic_sha_with_font_tiles(tile_specs: &[(u8, u8, u8, &[u8])]) -> ShaFile {
        const FLAG_FONT: u16 = 0x0001;
        parse_synthetic_sha_tileset(tile_specs, FLAG_FONT)
    }

    /// Builds and parses a minimal SHA payload with the given tileset `flags` value.
    fn parse_synthetic_sha_tileset(tile_specs: &[(u8, u8, u8, &[u8])], flags: u16) -> ShaFile {
        const TILESET_ENTRY_COUNT: usize = 128;
        const HEADER_BYTES: usize = TILESET_ENTRY_COUNT * 4 + TILESET_ENTRY_COUNT * 2;

        let tile_count =
            u8::try_from(tile_specs.len()).expect("synthetic fixture expects <=255 glyph tiles");
        let mut tileset_bytes = Vec::new();
        tileset_bytes.push(tile_count);
        tileset_bytes.extend_from_slice(&1u16.to_le_bytes());
        tileset_bytes.extend_from_slice(&0u16.to_le_bytes());
        tileset_bytes.extend_from_slice(&0u16.to_le_bytes());
        tileset_bytes.extend_from_slice(&0u16.to_le_bytes());
        tileset_bytes.push(8);
        tileset_bytes.extend_from_slice(&flags.to_le_bytes());

        for &(width, height, data_format, pixels) in tile_specs {
            assert_eq!(pixels.len(), usize::from(width) * usize::from(height));
            tileset_bytes.push(width);
            tileset_bytes.push(height);
            tileset_bytes.push(data_format);
            tileset_bytes.extend_from_slice(pixels);
        }

        let tileset_size =
            u16::try_from(tileset_bytes.len()).expect("synthetic tileset should fit u16 size");
        let tileset_offset = u32::try_from(HEADER_BYTES).expect("header fits u32");

        let mut bytes = vec![0_u8; HEADER_BYTES];
        bytes[0..4].copy_from_slice(&tileset_offset.to_le_bytes());
        let size_table_start = TILESET_ENTRY_COUNT * 4;
        bytes[size_table_start..size_table_start + 2].copy_from_slice(&tileset_size.to_le_bytes());
        bytes.extend_from_slice(&tileset_bytes);

        ShaFile::from_bytes(bytes).expect("synthetic SHA fixture should parse")
    }

    /// Verifies that `fill_rect_indexed` writes the correct region and leaves adjacent pixels
    /// untouched.
    #[test]
    fn fill_rect_indexed_fills_expected_region() {
        let mut fb = [0_u8; FRAMEBUFFER_PIXELS];
        fill_rect_indexed(&mut fb, 10, 20, 5, 3, 7);
        assert_eq!(
            fb[20 * FRAMEBUFFER_WIDTH + 10],
            7,
            "top-left corner must be filled"
        );
        assert_eq!(
            fb[22 * FRAMEBUFFER_WIDTH + 14],
            7,
            "bottom-right corner must be filled"
        );
        assert_eq!(
            fb[20 * FRAMEBUFFER_WIDTH + 15],
            0,
            "pixel just right of rect must be untouched"
        );
        assert_eq!(
            fb[23 * FRAMEBUFFER_WIDTH + 10],
            0,
            "pixel just below rect must be untouched"
        );
        assert_eq!(
            fb[19 * FRAMEBUFFER_WIDTH + 10],
            0,
            "pixel just above rect must be untouched"
        );
    }

    /// Verifies that `fill_rect_indexed` clips negative origin coordinates rather than panicking.
    #[test]
    fn fill_rect_indexed_clips_negative_coords() {
        let mut fb = [0_u8; FRAMEBUFFER_PIXELS];
        fill_rect_indexed(&mut fb, -2, -1, 4, 3, 3);
        assert_eq!(fb[0], 3, "clipped region starting at (0,0) must be filled");
        assert_eq!(
            fb[FRAMEBUFFER_WIDTH], 3,
            "second row of clipped region must be filled"
        );
    }

    /// Verifies that `fill_rect_indexed` is a no-op when width or height is zero.
    #[test]
    fn fill_rect_indexed_zero_dimension_fills_nothing() {
        let mut fb = [0_u8; FRAMEBUFFER_PIXELS];
        fill_rect_indexed(&mut fb, 0, 0, 0, 10, 5);
        fill_rect_indexed(&mut fb, 0, 0, 10, 0, 5);
        assert!(
            fb.iter().all(|&b| b == 0),
            "zero-dimension fill must leave framebuffer unchanged"
        );
    }

    /// Unit under test: `execute_commands_on_framebuffer` with `RenderCommand::Blit`.
    ///
    /// Preconditions: a synthetic SHA has one tileset at header entry 0 with one 1×1 tile
    /// containing pixel value 42.
    ///
    /// Invariants asserted: the framebuffer pixel at the blit destination contains 42 after
    /// dispatch, confirming tile lookup by entry index, pixel copying, and coordinate placement.
    #[test]
    fn execute_commands_on_framebuffer_blit_copies_tile_pixel() {
        let sha = parse_synthetic_sha_tileset(&[(1, 1, 0, &[42])], 0);
        let commands = [RenderCommand::Blit {
            tileset: 0,
            tile: 0,
            x: 5,
            y: 7,
            opaque: true,
            clip: None,
        }];
        let mut fb = [0_u8; FRAMEBUFFER_PIXELS];
        execute_commands_on_framebuffer(&mut fb, &commands, &sha);
        assert_eq!(
            fb[7 * FRAMEBUFFER_WIDTH + 5],
            42,
            "blit must copy tile pixel to destination"
        );
    }

    /// Unit under test: `execute_commands_on_framebuffer` with `RenderCommand::FillRect`.
    ///
    /// Preconditions: an empty (zero-header) SHA is supplied so no tile lookup is needed.
    ///
    /// Invariants asserted: the framebuffer region described by the command is filled with
    /// `color`, and pixels outside the region remain at their initial value of zero.
    #[test]
    fn execute_commands_on_framebuffer_fill_rect_fills_region() {
        const SHA_HEADER_BYTES: usize = 128 * 4 + 128 * 2;
        let sha =
            ShaFile::from_bytes(vec![0u8; SHA_HEADER_BYTES]).expect("zero SHA header should parse");
        let commands = [RenderCommand::FillRect {
            x: 2,
            y: 3,
            width: 4,
            height: 5,
            color: 9,
        }];
        let mut fb = [0_u8; FRAMEBUFFER_PIXELS];
        execute_commands_on_framebuffer(&mut fb, &commands, &sha);
        assert_eq!(
            fb[3 * FRAMEBUFFER_WIDTH + 2],
            9,
            "top-left corner must be filled"
        );
        assert_eq!(
            fb[7 * FRAMEBUFFER_WIDTH + 5],
            9,
            "bottom-right corner (x+w-1, y+h-1) must be filled"
        );
        assert_eq!(
            fb[3 * FRAMEBUFFER_WIDTH + 6],
            0,
            "pixel just right of rect must be untouched"
        );
        assert_eq!(
            fb[2 * FRAMEBUFFER_WIDTH + 2],
            0,
            "pixel just above rect must be untouched"
        );
    }

    /// Unit under test: `execute_commands_on_framebuffer` with
    /// `RenderCommand::DrawText`.
    ///
    /// Preconditions: a synthetic SHA carries 128 ASCII-indexed font
    /// glyphs whose `!` slot (codepoint 33) holds one non-zero pixel; the
    /// command requests logical color 5.
    ///
    /// Invariants asserted: the rendered glyph pixel uses
    /// `color_index + TEXT_COLOR_BRIGHT_SHIFT (8)`, mirroring
    /// `TextManager.initColorTextMap` in the Java reference: a logical
    /// foreground color is always rendered as the matching bright-EGA index.
    #[test]
    fn execute_commands_on_framebuffer_draw_text_applies_bright_ega_color() {
        let mut tiles = ascii_indexed_glyph_table();
        tiles[usize::from(b'!')] = (1, 1, 0, vec![9_u8]);
        let sha = parse_synthetic_sha_with_owned_font_tiles(&tiles);
        let commands = [RenderCommand::DrawText {
            text: "!".to_owned(),
            x: 4,
            y: 6,
            color_index: 5,
            font: FontSize::Small,
        }];
        let mut fb = [0_u8; FRAMEBUFFER_PIXELS];

        execute_commands_on_framebuffer(&mut fb, &commands, &sha);

        assert_eq!(
            fb[6 * FRAMEBUFFER_WIDTH + 4],
            5 + super::TEXT_COLOR_BRIGHT_SHIFT
        );
    }

    /// Unit under test: `execute_commands_on_framebuffer` with
    /// `RenderCommand::DrawText` and a logical color above 7.
    ///
    /// Preconditions: the command's `color_index` plus the bright-EGA shift
    /// would exceed 15.
    ///
    /// Invariants asserted: the resulting pixel index saturates at
    /// `BRIGHT_EGA_MAX (15)`, matching the
    /// `colorIndex > BACKGROUND_COLOR_WHITE` clamp in the Java reference.
    #[test]
    fn execute_commands_on_framebuffer_draw_text_clamps_above_bright_white() {
        let mut tiles = ascii_indexed_glyph_table();
        tiles[usize::from(b'!')] = (1, 1, 0, vec![9_u8]);
        let sha = parse_synthetic_sha_with_owned_font_tiles(&tiles);
        let commands = [RenderCommand::DrawText {
            text: "!".to_owned(),
            x: 4,
            y: 6,
            color_index: 12,
            font: FontSize::Small,
        }];
        let mut fb = [0_u8; FRAMEBUFFER_PIXELS];

        execute_commands_on_framebuffer(&mut fb, &commands, &sha);

        assert_eq!(fb[6 * FRAMEBUFFER_WIDTH + 4], super::BRIGHT_EGA_MAX);
    }
}
