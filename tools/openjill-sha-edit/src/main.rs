//! `openjill-sha-edit` — read-only egui viewer for Jill `*.SHA` tilesets.
//!
//! Ports the Java `sha-file-edit` Swing GUI on top of the shared
//! `openjill-ui` widgets and the `openjill-export::sha` renderer. v1 is
//! read-only: tilesets can be browsed and exported to PNG, but not mutated.

#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use openjill_core::{JILL_VGA_PALETTE, Palette};
use openjill_data::sha::ShaFile;
use openjill_export::sha::{TilesetColorOutput, tileset_to_png};
use openjill_ui::widgets::{
    FileTree, FileTreeState, PaletteFilter, PalettePicker, TileGrid, TileGridTexture,
};

/// Read-only viewer/exporter for Jill `*.SHA` tileset files.
#[derive(Debug, Parser)]
#[command(name = "openjill-sha-edit", version, about)]
struct Cli {
    /// SHA file to open on startup.
    #[arg(short, long)]
    file: Option<PathBuf>,
    /// Directory shown in the file browser (defaults to the file's parent or
    /// `OPENJILL_DATA_DIR`, then `data/original/JILL1`).
    #[arg(short, long)]
    data_dir: Option<PathBuf>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let data_dir = resolve_data_dir(cli.data_dir.as_deref(), cli.file.as_deref());
    let initial_file = cli.file.clone();

    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "openjill-sha-edit",
        options,
        Box::new(move |creation_context| {
            creation_context
                .egui_ctx
                .set_theme(egui::ThemePreference::System);
            Ok(Box::new(ShaEditApp::new(&data_dir, initial_file.clone())))
        }),
    )?;
    Ok(())
}

/// Resolves the file-browser root from the CLI flags and environment.
fn resolve_data_dir(explicit: Option<&Path>, file: Option<&Path>) -> PathBuf {
    if let Some(dir) = explicit {
        return dir.to_path_buf();
    }
    if let Some(parent) = file.and_then(Path::parent)
        && !parent.as_os_str().is_empty()
    {
        return parent.to_path_buf();
    }
    std::env::var("OPENJILL_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("data/original/JILL1"))
}

/// A parsed SHA file together with its source path.
struct LoadedSha {
    path: PathBuf,
    sha: ShaFile,
}

struct ShaEditApp {
    /// File-tree state filtered to `*.SHA`.
    file_tree_state: FileTreeState,
    /// Path most recently selected in the file tree.
    selected_file: Option<PathBuf>,
    /// Currently loaded SHA file, if any.
    loaded: Option<LoadedSha>,
    /// Index (into `sha.tilesets()`) of the selected tileset.
    selected_tileset: Option<usize>,
    /// GPU atlas for the selected tileset, keyed by its `entry_index`.
    tile_texture: Option<(usize, TileGridTexture)>,
    /// Selected tile within the preview grid.
    selected_tile: Option<usize>,
    /// Selected palette index in the palette picker.
    selected_palette_index: Option<u8>,
    /// Status / error line shown in the toolbar.
    status: String,
}

impl ShaEditApp {
    /// Builds the app, optionally loading `initial_file` immediately.
    fn new(data_dir: &Path, initial_file: Option<PathBuf>) -> Self {
        let file_tree_state = FileTreeState::new(data_dir).with_extensions(&["sha"]);
        let mut app = Self {
            file_tree_state,
            selected_file: None,
            loaded: None,
            selected_tileset: None,
            tile_texture: None,
            selected_tile: None,
            selected_palette_index: None,
            status: String::new(),
        };
        if let Some(path) = initial_file {
            app.load_file(&path);
            app.selected_file = Some(path);
        }
        app
    }

    /// Loads and parses a SHA file, resetting the current selection.
    fn load_file(&mut self, path: &Path) {
        match std::fs::read(path)
            .map_err(|e| e.to_string())
            .and_then(|bytes| ShaFile::from_bytes(bytes).map_err(|e| e.to_string()))
        {
            Ok(sha) => {
                let tileset_count = sha.tilesets().len();
                self.loaded = Some(LoadedSha {
                    path: path.to_path_buf(),
                    sha,
                });
                self.selected_tileset = (tileset_count > 0).then_some(0);
                self.tile_texture = None;
                self.selected_tile = None;
                self.status = format!("Loaded {} ({tileset_count} tilesets)", path.display());
            }
            Err(error) => {
                self.loaded = None;
                self.selected_tileset = None;
                self.tile_texture = None;
                self.status = format!("Failed to load {}: {error}", path.display());
            }
        }
    }

    /// Ensures `tile_texture` matches the selected tileset, building it from the
    /// wgpu render state when needed.
    fn ensure_texture(&mut self, render_state: Option<&eframe::egui_wgpu::RenderState>) {
        let (Some(loaded), Some(tileset_idx)) = (&self.loaded, self.selected_tileset) else {
            self.tile_texture = None;
            return;
        };
        let Some(tileset) = loaded.sha.tilesets().get(tileset_idx) else {
            self.tile_texture = None;
            return;
        };
        let key = tileset.entry_index();
        if self.tile_texture.as_ref().is_some_and(|(k, _)| *k == key) {
            return;
        }
        if let Some(render_state) = render_state {
            let palette = Palette::jill_vga();
            let texture = TileGridTexture::from_tileset(render_state, tileset, &palette);
            self.tile_texture = Some((key, texture));
        }
    }

    /// Exports the selected tileset to `tileset_<index>.png` next to the SHA
    /// file (or in the current directory), reporting the result in `status`.
    fn export_selected(&mut self) {
        let (Some(loaded), Some(tileset_idx)) = (&self.loaded, self.selected_tileset) else {
            self.status = "No tileset selected to export".to_string();
            return;
        };
        let Some(tileset) = loaded.sha.tilesets().get(tileset_idx) else {
            return;
        };
        let output = TilesetColorOutput::Colored {
            palette: Arc::new(JILL_VGA_PALETTE),
        };
        let image = tileset_to_png(tileset, output);
        let dir = loaded.path.parent().unwrap_or_else(|| Path::new("."));
        let path = dir.join(format!("tileset_{}.png", tileset.entry_index()));
        match image.save(&path) {
            Ok(()) => self.status = format!("Exported {}", path.display()),
            Err(error) => self.status = format!("Export failed: {error}"),
        }
    }
}

impl eframe::App for ShaEditApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let render_state = frame.wgpu_render_state();

        // Toolbar.
        egui::Panel::top("sha_edit_toolbar").show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Export selection").clicked() {
                    self.export_selected();
                }
                ui.separator();
                ui.label(if self.status.is_empty() {
                    "Open a .SHA file from the tree"
                } else {
                    self.status.as_str()
                });
            });
        });

        // Left: file tree filtered to *.SHA.
        egui::Panel::left("sha_edit_files")
            .resizable(true)
            .default_size(220.0)
            .show_inside(ui, |ui| {
                ui.heading("Files");
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let before = self.selected_file.clone();
                    FileTree::new(&mut self.file_tree_state, &mut self.selected_file).show(ui);
                    if self.selected_file != before
                        && let Some(path) = self.selected_file.clone()
                    {
                        self.load_file(&path);
                    }
                });
            });

        // Right: selected-tile preview + palette.
        egui::Panel::right("sha_edit_preview")
            .resizable(true)
            .default_size(320.0)
            .show_inside(ui, |ui| {
                ui.heading("Preview");
                self.ensure_texture(render_state);
                match &self.tile_texture {
                    Some((_, texture)) => {
                        egui::ScrollArea::both().show(ui, |ui| {
                            TileGrid::new(texture, &mut self.selected_tile)
                                .columns(8)
                                .zoom(2.0)
                                .hover_highlight(true)
                                .show(ui);
                        });
                        ui.label(format!("Selected tile: {:?}", self.selected_tile));
                    }
                    None => {
                        ui.label("Select a tileset to preview its tiles.");
                    }
                }
                ui.separator();
                ui.label("Palette (Jill VGA)");
                PalettePicker::new(&JILL_VGA_PALETTE, &mut self.selected_palette_index)
                    .filter(PaletteFilter::All)
                    .swatch_size(12.0)
                    .show(ui);
            });

        // Centre: tileset list.
        ui.heading("Tilesets");
        match &self.loaded {
            Some(loaded) => {
                let count = loaded.sha.tilesets().len();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for index in 0..count {
                        let tileset = &loaded.sha.tilesets()[index];
                        let label = format!(
                            "#{:<3} {} tiles  {:>5} B  {} bpp{}",
                            tileset.entry_index(),
                            tileset.tile_count(),
                            tileset.size(),
                            tileset.bit_depth(),
                            if tileset.is_font() { "  [font]" } else { "" },
                        );
                        if ui
                            .selectable_label(self.selected_tileset == Some(index), label)
                            .clicked()
                            && self.selected_tileset != Some(index)
                        {
                            self.selected_tileset = Some(index);
                            self.selected_tile = None;
                            self.tile_texture = None;
                        }
                    }
                });
            }
            None => {
                ui.label("No SHA file loaded.");
            }
        }
    }
}
