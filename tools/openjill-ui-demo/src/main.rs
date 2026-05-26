#![forbid(unsafe_code)]

use anyhow::Result;
use openjill_core::Palette;
use openjill_data::sha::ShaFile;
use openjill_ui::widgets::{TileGrid, TileGridTexture};
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
    tile_grid: Option<TileGridTexture>,
    selected_tile: Option<usize>,
    last_clicked_tile: Option<usize>,
    status: String,
}

impl DemoApp {
    fn new(creation_context: &eframe::CreationContext<'_>) -> Self {
        match load_tileset_texture(creation_context) {
            Ok(tile_grid) => Self {
                tile_grid: Some(tile_grid),
                selected_tile: None,
                last_clicked_tile: None,
                status: String::new(),
            },
            Err(error) => Self {
                tile_grid: None,
                selected_tile: None,
                last_clicked_tile: None,
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
    }
}

fn load_tileset_texture(creation_context: &eframe::CreationContext<'_>) -> Result<TileGridTexture, String> {
    let render_state = creation_context
        .wgpu_render_state
        .as_ref()
        .ok_or_else(|| "wgpu render state unavailable".to_string())?;
    let data_dir = std::env::var("OPENJILL_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("data/original/JILL1"));
    let sha_path = resolve_sha_path(&data_dir);
    let bytes = fs::read(&sha_path).map_err(|error| format!("failed to read {}: {error}", sha_path.display()))?;
    let sha = ShaFile::from_bytes(bytes)
        .map_err(|error| format!("failed to parse {}: {error}", sha_path.display()))?;
    let tileset = sha
        .tilesets()
        .iter()
        .find(|tileset| tileset.entry_index() == 24)
        .ok_or_else(|| "tileset 24 not found in JILL1.SHA".to_string())?;
    let palette = Palette::jill_vga();
    Ok(TileGridTexture::from_tileset(render_state, tileset, &palette))
}

fn resolve_sha_path(data_dir: &Path) -> PathBuf {
    let candidate = data_dir.join("JILL1.SHA");
    if candidate.exists() {
        return candidate;
    }
    data_dir.to_path_buf()
}
