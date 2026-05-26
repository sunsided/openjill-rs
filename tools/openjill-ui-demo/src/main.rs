#![forbid(unsafe_code)]

use anyhow::Result;
use openjill_core::{JILL_VGA_PALETTE, Palette};
use openjill_data::sha::ShaFile;
use openjill_ui::widgets::{PaletteFilter, PalettePicker, TileGrid, TileGridTexture};
use std::fs;
use std::path::{Path, PathBuf};

fn main() -> Result<()> {
    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "openjill-ui-demo",
        options,
        Box::new(|creation_context| {
            creation_context
                .egui_ctx
                .set_theme(egui::ThemePreference::System);
            Ok(Box::new(DemoApp::new(creation_context)))
        }),
    )?;
    Ok(())
}

struct DemoApp {
    /// Loaded TileGrid texture atlas for `JILL1.SHA` tileset 24.
    tile_grid: Option<TileGridTexture>,
    /// Currently selected tile index.
    selected_tile: Option<usize>,
    /// Most recent click event tile index emitted by the widget.
    last_clicked_tile: Option<usize>,
    /// Currently selected palette index.
    selected_palette_index: Option<u8>,
    /// Most recent click event palette index emitted by the widget.
    last_clicked_palette_index: Option<u8>,
    /// Visible palette slice in the demo.
    palette_filter: PaletteFilter,
    /// Status/error message shown when loading fails.
    status: String,
}

impl DemoApp {
    /// Constructs the demo app and attempts to load tileset 24 from `JILL1.SHA`.
    fn new(creation_context: &eframe::CreationContext<'_>) -> Self {
        match load_tileset_texture(creation_context) {
            Ok(tile_grid) => Self {
                tile_grid: Some(tile_grid),
                selected_tile: None,
                last_clicked_tile: None,
                selected_palette_index: None,
                last_clicked_palette_index: None,
                palette_filter: PaletteFilter::All,
                status: String::new(),
            },
            Err(error) => Self {
                tile_grid: None,
                selected_tile: None,
                last_clicked_tile: None,
                selected_palette_index: None,
                last_clicked_palette_index: None,
                palette_filter: PaletteFilter::All,
                status: error,
            },
        }
    }
}

impl eframe::App for DemoApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui.heading("openjill_ui::widgets::TileGrid");
        ui.label("JILL1.SHA / tileset 24");

        if let Some(grid) = &self.tile_grid {
            let output = TileGrid::new(grid, &mut self.selected_tile)
                .columns(8)
                .zoom(2.0)
                .hover_highlight(true)
                .show(ui);
            if let Some(index) = output.clicked_tile {
                self.last_clicked_tile = Some(index);
            }
            ui.label(format!("Selected tile: {:?}", self.selected_tile));
            ui.label(format!("Last click event: {:?}", self.last_clicked_tile));
        } else {
            ui.colored_label(ui.visuals().warn_fg_color, &self.status);
        }

        ui.separator();
        ui.heading("openjill_ui::widgets::PalettePicker");
        egui::ComboBox::from_label("Palette slice")
            .selected_text(self.palette_filter.label())
            .show_ui(ui, |ui| {
                for filter in PaletteFilter::ALL {
                    ui.selectable_value(&mut self.palette_filter, filter, filter.label());
                }
            });
        let output = PalettePicker::new(&JILL_VGA_PALETTE, &mut self.selected_palette_index)
            .filter(self.palette_filter)
            .show(ui);
        if let Some(index) = output.clicked_index {
            self.last_clicked_palette_index = Some(index);
        }
        ui.label(format!(
            "Selected palette index: {:?}",
            self.selected_palette_index
        ));
        ui.label(format!(
            "Last palette click event: {:?}",
            self.last_clicked_palette_index
        ));
    }
}

/// Loads tileset 24 from `JILL1.SHA` into a GPU-backed [`TileGridTexture`].
///
/// The loader resolves the data directory from `OPENJILL_DATA_DIR`, falling
/// back to `data/original/JILL1`.
fn load_tileset_texture(
    creation_context: &eframe::CreationContext<'_>,
) -> Result<TileGridTexture, String> {
    let render_state = creation_context
        .wgpu_render_state
        .as_ref()
        .ok_or_else(|| "wgpu render state unavailable".to_string())?;
    let data_dir = std::env::var("OPENJILL_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("data/original/JILL1"));
    let sha_path = resolve_sha_path(&data_dir)?;
    let bytes = fs::read(&sha_path)
        .map_err(|error| format!("failed to read {}: {error}", sha_path.display()))?;
    let sha = ShaFile::from_bytes(bytes)
        .map_err(|error| format!("failed to parse {}: {error}", sha_path.display()))?;
    let tileset = sha
        .tilesets()
        .iter()
        .find(|tileset| tileset.entry_index() == 24)
        .ok_or_else(|| "tileset 24 not found in JILL1.SHA".to_string())?;
    let palette = Palette::jill_vga();
    Ok(TileGridTexture::from_tileset(
        render_state,
        tileset,
        &palette,
    ))
}

/// Resolves the expected `JILL1.SHA` file path under a data directory.
///
/// Returns a descriptive error when the file is missing.
fn resolve_sha_path(data_dir: &Path) -> Result<PathBuf, String> {
    let candidate = data_dir.join("JILL1.SHA");
    if candidate.exists() {
        return Ok(candidate);
    }
    Err(format!(
        "failed to locate JILL1.SHA at {} (set OPENJILL_DATA_DIR to the Jill data directory)",
        candidate.display()
    ))
}
