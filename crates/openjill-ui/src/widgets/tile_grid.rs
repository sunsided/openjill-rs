use eframe::CreationContext;
use egui::{Color32, Rect, Response, Sense, Stroke, StrokeKind, TextureId, Ui, Vec2};
use egui_wgpu::RenderState;
use openjill_data::sha::ShaTileSet;
use openjill_render::{Palette, expand_indexed_pixels};

/// GPU-backed texture atlas for a SHA tileset used by [`TileGrid`].
pub struct TileGridTexture {
    /// Egui texture handle referencing the registered atlas texture.
    texture_id: TextureId,
    /// Number of tiles available in this atlas.
    tile_count: usize,
    /// Uniform tile cell size in atlas texels `[width, height]`.
    cell_size: [u32; 2],
    /// Owned GPU texture backing the egui texture registration.
    _texture: egui_wgpu::wgpu::Texture,
}

impl TileGridTexture {
    /// Builds a GPU texture atlas for one SHA tileset.
    pub fn from_tileset(render_state: &RenderState, tileset: &ShaTileSet, palette: &Palette) -> Self {
        let cell_width = tileset
            .tiles()
            .iter()
            .map(|tile| u32::from(tile.width()))
            .max()
            .unwrap_or(1)
            .max(1);
        let cell_height = tileset
            .tiles()
            .iter()
            .map(|tile| u32::from(tile.height()))
            .max()
            .unwrap_or(1)
            .max(1);
        let tile_count = tileset.tiles().len();
        let tile_count_u32 =
            u32::try_from(tile_count).expect("tile count must fit into u32 for GPU texture atlas");
        let atlas_width = cell_width.saturating_mul(tile_count_u32).max(1);
        let atlas_height = cell_height.max(1);
        let max_texture_dimension = render_state.device.limits().max_texture_dimension_2d;
        assert!(
            atlas_width <= max_texture_dimension && atlas_height <= max_texture_dimension,
            "tile atlas dimensions must fit GPU max texture size"
        );
        let mut atlas_rgba = vec![0_u8; (atlas_width as usize) * (atlas_height as usize) * 4];

        for (tile_index, tile) in tileset.tiles().iter().enumerate() {
            let tile_width = u32::from(tile.width());
            let tile_height = u32::from(tile.height());
            if tile_width == 0 || tile_height == 0 {
                continue;
            }
            let expected_pixels = (tile_width as usize) * (tile_height as usize);
            if tile.indexed_pixels().len() < expected_pixels {
                continue;
            }

            let mut tile_rgba = vec![0_u8; expected_pixels * 4];
            expand_indexed_pixels(tile.indexed_pixels(), &mut tile_rgba, palette);
            let dst_x = (tile_index as u32) * cell_width;

            for row in 0..tile_height as usize {
                let dst_row_start = (((row as u32) * atlas_width + dst_x) as usize) * 4;
                let src_row_start = row * (tile_width as usize) * 4;
                let row_bytes = (tile_width as usize) * 4;
                atlas_rgba[dst_row_start..dst_row_start + row_bytes]
                    .copy_from_slice(&tile_rgba[src_row_start..src_row_start + row_bytes]);
            }
        }

        let texture = render_state
            .device
            .create_texture(&egui_wgpu::wgpu::TextureDescriptor {
                label: Some("openjill-ui-tile-grid-texture"),
                size: egui_wgpu::wgpu::Extent3d {
                    width: atlas_width,
                    height: atlas_height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: egui_wgpu::wgpu::TextureDimension::D2,
                format: egui_wgpu::wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: egui_wgpu::wgpu::TextureUsages::TEXTURE_BINDING
                    | egui_wgpu::wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
        let bytes_per_row = atlas_width * 4;
        let align = egui_wgpu::wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_bytes_per_row = bytes_per_row.div_ceil(align) * align;
        let upload_data = if padded_bytes_per_row == bytes_per_row {
            atlas_rgba
        } else {
            let mut padded = vec![0_u8; (padded_bytes_per_row * atlas_height) as usize];
            let bytes_per_row = bytes_per_row as usize;
            let padded_bytes_per_row = padded_bytes_per_row as usize;
            for row in 0..atlas_height as usize {
                let src_start = row * bytes_per_row;
                let src_end = src_start + bytes_per_row;
                let dst_start = row * padded_bytes_per_row;
                padded[dst_start..dst_start + bytes_per_row]
                    .copy_from_slice(&atlas_rgba[src_start..src_end]);
            }
            padded
        };
        render_state.queue.write_texture(
            egui_wgpu::wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: egui_wgpu::wgpu::Origin3d::ZERO,
                aspect: egui_wgpu::wgpu::TextureAspect::All,
            },
            &upload_data,
            egui_wgpu::wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_bytes_per_row),
                rows_per_image: Some(atlas_height),
            },
            egui_wgpu::wgpu::Extent3d {
                width: atlas_width,
                height: atlas_height,
                depth_or_array_layers: 1,
            },
        );
        let texture_view = texture.create_view(&egui_wgpu::wgpu::TextureViewDescriptor::default());
        let texture_id = render_state.renderer.write().register_native_texture(
            &render_state.device,
            &texture_view,
            egui_wgpu::wgpu::FilterMode::Nearest,
        );

        Self {
            texture_id,
            tile_count,
            cell_size: [cell_width, cell_height],
            _texture: texture,
        }
    }

    /// Creates a texture atlas from eframe's WGPU creation context.
    pub fn from_creation_context(
        context: &CreationContext<'_>,
        tileset: &ShaTileSet,
        palette: &Palette,
    ) -> Option<Self> {
        context
            .wgpu_render_state
            .as_ref()
            .map(|state| Self::from_tileset(state, tileset, palette))
    }
}

/// Output from [`TileGrid::show`].
pub struct TileGridOutput {
    /// Combined egui response produced from all tile interactions in this grid.
    pub response: Response,
    /// Tile index that was clicked this frame.
    pub clicked_tile: Option<usize>,
}

/// Egui widget that renders a clickable uniform tile grid from a tileset texture atlas.
pub struct TileGrid<'a> {
    /// Texture atlas to display.
    texture: &'a TileGridTexture,
    /// Mutable selected tile index updated when a tile is clicked.
    selected: &'a mut Option<usize>,
    /// Number of tiles per row.
    columns: usize,
    /// Per-tile scale factor.
    zoom: f32,
    /// Whether hovered tiles receive a highlight outline.
    hover_highlight: bool,
}

impl<'a> TileGrid<'a> {
    /// Creates a tile grid widget for a texture atlas and mutable selection.
    pub fn new(texture: &'a TileGridTexture, selected: &'a mut Option<usize>) -> Self {
        Self {
            texture,
            selected,
            columns: 8,
            zoom: 1.0,
            hover_highlight: true,
        }
    }

    /// Sets grid columns.
    pub fn columns(mut self, columns: usize) -> Self {
        self.columns = columns.max(1);
        self
    }

    /// Sets tile zoom factor.
    pub fn zoom(mut self, zoom: f32) -> Self {
        self.zoom = zoom.max(0.1);
        self
    }

    /// Enables/disables hover highlight.
    pub fn hover_highlight(mut self, enabled: bool) -> Self {
        self.hover_highlight = enabled;
        self
    }

    /// Adds the widget to a UI and returns response plus clicked tile index.
    pub fn show(self, ui: &mut Ui) -> TileGridOutput {
        if self.texture.tile_count == 0 {
            return TileGridOutput {
                response: ui.allocate_response(Vec2::ZERO, Sense::hover()),
                clicked_tile: None,
            };
        }

        let columns = self.columns;
        let rows = self.texture.tile_count.div_ceil(columns);
        let tile_size = Vec2::new(
            self.texture.cell_size[0] as f32 * self.zoom,
            self.texture.cell_size[1] as f32 * self.zoom,
        );
        let grid_size = Vec2::new(tile_size.x * columns as f32, tile_size.y * rows as f32);
        let (grid_rect, mut response) = ui.allocate_exact_size(grid_size, Sense::click());
        if !ui.is_rect_visible(grid_rect) {
            return TileGridOutput {
                response,
                clicked_tile: None,
            };
        }

        let mut clicked_tile = None;
        for tile_index in 0..self.texture.tile_count {
            let col = tile_index % columns;
            let row = tile_index / columns;
            let tile_rect = Rect::from_min_size(
                grid_rect.min + Vec2::new(col as f32 * tile_size.x, row as f32 * tile_size.y),
                tile_size,
            );
            let tile_response = ui.put(
                tile_rect,
                egui::Image::new((self.texture.texture_id, tile_size))
                    .uv(tile_uv(tile_index, self.texture.tile_count))
                    .sense(Sense::click()),
            );

            if tile_response.clicked() {
                clicked_tile = Some(tile_index);
                if *self.selected != Some(tile_index) {
                    *self.selected = Some(tile_index);
                    response.mark_changed();
                }
            }

            if self.hover_highlight && tile_response.hovered() {
                ui.painter().rect_stroke(
                    tile_rect.shrink(0.5),
                    0.0,
                    Stroke::new(1.0, Color32::WHITE),
                    StrokeKind::Inside,
                );
            }
            if *self.selected == Some(tile_index) {
                ui.painter().rect_stroke(
                    tile_rect.shrink(0.5),
                    0.0,
                    Stroke::new(2.0, ui.visuals().selection.stroke.color),
                    StrokeKind::Inside,
                );
            }

            response = response.union(tile_response);
        }

        TileGridOutput {
            response,
            clicked_tile,
        }
    }
}

impl egui::Widget for TileGrid<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        self.show(ui).response
    }
}

/// Computes normalized UV coordinates for one tile in a single-row atlas layout.
fn tile_uv(tile_index: usize, tile_count: usize) -> Rect {
    let tile_count = tile_count.max(1) as f32;
    let min_x = tile_index as f32 / tile_count;
    let max_x = (tile_index + 1) as f32 / tile_count;
    Rect::from_min_max(egui::pos2(min_x, 0.0), egui::pos2(max_x, 1.0))
}

#[cfg(test)]
mod tests {
    use super::tile_uv;

    #[test]
    fn tile_uv_splits_single_row_atlas_evenly() {
        assert_eq!(
            tile_uv(1, 4),
            egui::Rect::from_min_max(egui::pos2(0.25, 0.0), egui::pos2(0.5, 1.0))
        );
    }
}
