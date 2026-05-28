//! `openjill-jn-view` — read-only egui level viewer for Jill `*.JN?` maps.
//!
//! Renders a JN map (via `openjill-export::jn`) into an egui texture and shows
//! it in a scrollable, zoomable canvas with an object overlay, an object list,
//! and a per-object inspector. The viewer is read-only.

#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

use anyhow::Result;
use clap::Parser;
use egui::load::SizedTexture;
use image::RgbaImage;
use openjill_core::Palette;
use openjill_data::dma::DmaFile;
use openjill_data::episode::{self, Episode};
use openjill_data::jn::JnFile;
use openjill_data::sha::ShaFile;
use openjill_export::jn::map_to_png;
use openjill_ui::widgets::{FileTree, FileTreeState};

/// Shared tile-metadata file name (identical across episodes).
const SHARED_DMA_FILE: &str = "JILL.DMA";

/// Read-only GUI level viewer for Jill `*.JN?` map files.
#[derive(Debug, Parser)]
#[command(name = "openjill-jn-view", version, about)]
struct Cli {
    /// JN map file to open on startup.
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
        "openjill-jn-view",
        options,
        Box::new(move |creation_context| {
            creation_context
                .egui_ctx
                .set_theme(egui::ThemePreference::System);
            Ok(Box::new(JnViewApp::new(&data_dir, initial_file.clone())))
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

/// A loaded JN map with its rendered image and GPU texture.
struct LoadedJn {
    path: PathBuf,
    jn: JnFile,
    image: RgbaImage,
    texture: egui::TextureHandle,
}

struct JnViewApp {
    /// File-tree state filtered to `*.JN?`.
    file_tree_state: FileTreeState,
    /// Path most recently selected in the file tree.
    selected_file: Option<PathBuf>,
    /// Currently loaded map, if any.
    loaded: Option<LoadedJn>,
    /// Selected object index (into `jn.objects()`).
    selected_object: Option<usize>,
    /// Canvas zoom factor.
    zoom: f32,
    /// Whether the object overlay is drawn.
    show_overlay: bool,
    /// Status / error line.
    status: String,
}

impl JnViewApp {
    /// Builds the app, optionally loading `initial_file` immediately.
    fn new(data_dir: &Path, initial_file: Option<PathBuf>) -> Self {
        let file_tree_state = FileTreeState::new(data_dir).with_extensions(&["jn1", "jn2", "jn3"]);
        Self {
            file_tree_state,
            selected_file: initial_file,
            loaded: None,
            selected_object: None,
            zoom: 1.0,
            show_overlay: true,
            status: String::new(),
        }
    }

    /// Loads and renders a JN map, resolving the SHA/DMA next to it.
    fn load_file(&mut self, ctx: &egui::Context, path: &Path) {
        match render_map(path) {
            Ok((jn, image)) => {
                let size = [image.width() as usize, image.height() as usize];
                let color = egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw());
                let texture = ctx.load_texture("jn-map", color, egui::TextureOptions::NEAREST);
                self.status = format!(
                    "Loaded {} ({} objects, {}x{})",
                    path.display(),
                    jn.objects().len(),
                    image.width(),
                    image.height()
                );
                self.loaded = Some(LoadedJn {
                    path: path.to_path_buf(),
                    jn,
                    image,
                    texture,
                });
                self.selected_object = None;
            }
            Err(error) => {
                self.loaded = None;
                self.selected_object = None;
                self.status = format!("Failed to load {}: {error}", path.display());
            }
        }
    }

    /// Exports the current map image to `<jn-name>.png` next to the source.
    fn export_view(&mut self) {
        let Some(loaded) = &self.loaded else {
            self.status = "No map loaded to export".to_string();
            return;
        };
        let dir = loaded.path.parent().unwrap_or_else(|| Path::new("."));
        let name = loaded
            .path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("map");
        let out = dir.join(format!("{name}.png"));
        match loaded.image.save(&out) {
            Ok(()) => self.status = format!("Exported {}", out.display()),
            Err(error) => self.status = format!("Export failed: {error}"),
        }
    }
}

impl eframe::App for JnViewApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // Deferred initial / tree-driven load.
        if self.loaded.as_ref().map(|l| &l.path) != self.selected_file.as_ref()
            && let Some(path) = self.selected_file.clone()
        {
            self.load_file(&ctx, &path);
        }

        // Toolbar.
        egui::Panel::top("jn_view_toolbar").show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Zoom");
                ui.add(egui::Slider::new(&mut self.zoom, 0.25..=4.0).step_by(0.25));
                ui.checkbox(&mut self.show_overlay, "Object overlay");
                if ui.button("Export PNG").clicked() {
                    self.export_view();
                }
                ui.separator();
                ui.label(if self.status.is_empty() {
                    "Open a .JN? map from the tree"
                } else {
                    self.status.as_str()
                });
            });
        });

        // Left: file tree.
        egui::Panel::left("jn_view_files")
            .resizable(true)
            .default_size(220.0)
            .show_inside(ui, |ui| {
                ui.heading("Maps");
                egui::ScrollArea::vertical().show(ui, |ui| {
                    FileTree::new(&mut self.file_tree_state, &mut self.selected_file).show(ui);
                });
            });

        // Right: object list + inspector.
        egui::Panel::right("jn_view_objects")
            .resizable(true)
            .default_size(280.0)
            .show_inside(ui, |ui| {
                ui.heading("Objects");
                match &self.loaded {
                    Some(loaded) => {
                        let count = loaded.jn.objects().len();
                        egui::ScrollArea::vertical()
                            .max_height(240.0)
                            .show(ui, |ui| {
                                for index in 0..count {
                                    let object = &loaded.jn.objects()[index];
                                    let label = format!(
                                        "#{:<3} type {:<3} ({}, {})",
                                        index,
                                        object.object_type(),
                                        object.x(),
                                        object.y()
                                    );
                                    ui.selectable_value(
                                        &mut self.selected_object,
                                        Some(index),
                                        label,
                                    );
                                }
                            });
                        ui.separator();
                        if let Some(object) = self
                            .selected_object
                            .and_then(|i| loaded.jn.objects().get(i))
                        {
                            egui::Grid::new("jn_object_inspector")
                                .num_columns(2)
                                .show(ui, |ui| {
                                    inspector_row(ui, "type", object.object_type());
                                    inspector_row(ui, "x", object.x());
                                    inspector_row(ui, "y", object.y());
                                    inspector_row(ui, "x_speed", object.x_speed());
                                    inspector_row(ui, "y_speed", object.y_speed());
                                    inspector_row(ui, "counter", object.counter());
                                    inspector_row(ui, "info1", object.info1());
                                });
                        } else {
                            ui.label("Select an object to inspect.");
                        }
                    }
                    None => {
                        ui.label("No map loaded.");
                    }
                }
            });

        // Centre: scrollable, zoomable map canvas with object overlay.
        match &self.loaded {
            Some(loaded) => {
                egui::ScrollArea::both().show(ui, |ui| {
                    let size = egui::vec2(
                        loaded.image.width() as f32 * self.zoom,
                        loaded.image.height() as f32 * self.zoom,
                    );
                    let texture = SizedTexture::new(loaded.texture.id(), size);
                    let response = ui.add(egui::Image::new(texture));
                    if self.show_overlay {
                        draw_object_overlay(
                            ui,
                            response.rect,
                            self.zoom,
                            loaded,
                            self.selected_object,
                        );
                    }
                });
            }
            None => {
                ui.centered_and_justified(|ui| {
                    ui.label("Open a .JN? map from the file tree.");
                });
            }
        }
    }
}

/// Renders a single inspector key/value row.
fn inspector_row(ui: &mut egui::Ui, label: &str, value: impl std::fmt::Display) {
    ui.label(label);
    ui.label(value.to_string());
    ui.end_row();
}

/// Draws object markers over the map image, highlighting the selected object.
fn draw_object_overlay(
    ui: &egui::Ui,
    image_rect: egui::Rect,
    zoom: f32,
    loaded: &LoadedJn,
    selected: Option<usize>,
) {
    let painter = ui.painter_at(image_rect);
    for (index, object) in loaded.jn.objects().iter().enumerate() {
        let center =
            image_rect.min + egui::vec2(object.x() as f32 * zoom, object.y() as f32 * zoom);
        let is_selected = selected == Some(index);
        let radius = if is_selected { 5.0 } else { 3.0 };
        let color = if is_selected {
            egui::Color32::YELLOW
        } else {
            egui::Color32::from_rgba_unmultiplied(255, 80, 80, 180)
        };
        painter.circle_stroke(center, radius, egui::Stroke::new(1.5, color));
    }
}

/// Reads, parses, and renders a JN map, returning the parsed file and image.
fn render_map(path: &Path) -> Result<(JnFile, RgbaImage), String> {
    let jn = JnFile::from_bytes(std::fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let data_dir = path.parent().unwrap_or_else(|| Path::new("."));
    let episode = episode_for(path);
    let sha =
        ShaFile::from_bytes(std::fs::read(data_dir.join(episode.sha)).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let dma = DmaFile::from_bytes(
        std::fs::read(data_dir.join(SHARED_DMA_FILE)).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let palette = Palette::jill_vga();
    let image = map_to_png(&jn, &sha, &dma, &palette);
    Ok((jn, image))
}

/// Resolves the episode descriptor from the JN file extension.
fn episode_for(jn_path: &Path) -> &'static Episode {
    jn_path
        .extension()
        .and_then(|ext| ext.to_str())
        .and_then(Episode::from_jn_extension)
        .unwrap_or(&episode::JILL1)
}
