//! `openjill jn` subcommands: render `*.JN?` maps to PNG / dump objects, and a
//! read-only GUI level viewer.
//!
//! Ports the former `openjill-jn-extract` (map -> PNG, object dump) and
//! `openjill-jn-view` (egui level viewer) tools onto the `openjill-data` parser
//! and the `openjill-export::jn` renderer. Rendering a map needs the episode
//! SHA tileset and the shared `JILL.DMA`; both are resolved next to the JN file
//! unless overridden.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use openjill_core::Palette;
use openjill_core::entity::Rect;
use openjill_data::dma::DmaFile;
use openjill_data::episode::{self, Episode};
use openjill_data::jn::JnFile;
use openjill_data::sha::ShaFile;
use openjill_export::jn::map_to_png_with_viewport;
#[cfg(feature = "editor-ui")]
use view::ViewArgs;

/// Shared tile-metadata file name (identical across episodes).
const SHARED_DMA_FILE: &str = "JILL.DMA";

/// Actions for the `openjill jn` subcommand.
#[derive(Debug, Subcommand)]
pub enum Action {
    /// Render a map to PNG (or dump its object layer with `--objects`).
    Extract(ExtractArgs),
    /// Open the read-only level viewer GUI.
    #[cfg(feature = "editor-ui")]
    View(ViewArgs),
}

/// Arguments for `openjill jn extract`.
#[derive(Args, Debug)]
pub struct ExtractArgs {
    /// JN map file to read (e.g. `1.JN1`).
    #[arg(short, long)]
    file: PathBuf,
    /// Output PNG path (defaults to the JN file name with a `.png` extension).
    #[arg(short, long)]
    out: Option<PathBuf>,
    /// SHA tileset file (defaults to the episode SHA next to the JN file).
    #[arg(short, long)]
    sha: Option<PathBuf>,
    /// DMA tile-metadata file (defaults to `JILL.DMA` next to the JN file).
    #[arg(short, long)]
    dma: Option<PathBuf>,
    /// Clip the render to a viewport rectangle in map-pixel space.
    #[arg(long, num_args = 4, value_names = ["X", "Y", "W", "H"])]
    viewport: Option<Vec<i32>>,
    /// Dump the object layer as text instead of rendering a PNG.
    #[arg(long)]
    objects: bool,
}

/// Runs `openjill jn <action>`.
pub fn run(action: Action) -> Result<()> {
    match action {
        Action::Extract(args) => extract_command(args),
        #[cfg(feature = "editor-ui")]
        Action::View(args) => view::view_command(args),
    }
}

/// Renders a JN map to PNG, or dumps its object layer with `--objects`.
fn extract_command(args: ExtractArgs) -> Result<()> {
    let jn_bytes = std::fs::read(&args.file)
        .with_context(|| format!("failed to read JN file {}", args.file.display()))?;
    let jn = JnFile::from_bytes(jn_bytes)
        .with_context(|| format!("failed to parse JN file {}", args.file.display()))?;

    if args.objects {
        print!("{}", objects_text(&jn));
        return Ok(());
    }

    let data_dir = args.file.parent().unwrap_or_else(|| Path::new("."));
    let episode = episode_for(&args.file);
    let sha_path = args
        .sha
        .clone()
        .unwrap_or_else(|| data_dir.join(episode.sha));
    let dma_path = args
        .dma
        .clone()
        .unwrap_or_else(|| data_dir.join(SHARED_DMA_FILE));

    let sha = ShaFile::from_bytes(
        std::fs::read(&sha_path)
            .with_context(|| format!("failed to read SHA file {}", sha_path.display()))?,
    )
    .with_context(|| format!("failed to parse SHA file {}", sha_path.display()))?;
    let dma = DmaFile::from_bytes(
        std::fs::read(&dma_path)
            .with_context(|| format!("failed to read DMA file {}", dma_path.display()))?,
    )
    .with_context(|| format!("failed to parse DMA file {}", dma_path.display()))?;

    let viewport = parse_viewport(args.viewport.as_deref())?;
    let palette = Palette::jill_vga();
    let image = map_to_png_with_viewport(&jn, &sha, &dma, &palette, viewport);
    if image.width() == 0 || image.height() == 0 {
        bail!("viewport does not intersect the map; nothing to render");
    }

    let out_path = args
        .out
        .clone()
        .unwrap_or_else(|| default_png_path(&args.file));
    image
        .save(&out_path)
        .with_context(|| format!("failed to write {}", out_path.display()))?;
    println!(
        "Wrote {}x{} map to {}",
        image.width(),
        image.height(),
        out_path.display()
    );
    Ok(())
}

/// Resolves the episode descriptor from the JN file extension, defaulting to
/// episode 1 when the extension is unrecognised.
fn episode_for(jn_path: &Path) -> &'static Episode {
    jn_path
        .extension()
        .and_then(|ext| ext.to_str())
        .and_then(Episode::from_jn_extension)
        .unwrap_or(&episode::JILL1)
}

/// Builds the default output PNG path from the JN file name.
fn default_png_path(jn_path: &Path) -> PathBuf {
    let stem = jn_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("map");
    PathBuf::from(format!("{stem}.png"))
}

/// Converts the optional four-value `--viewport` argument into a [`Rect`].
fn parse_viewport(values: Option<&[i32]>) -> Result<Option<Rect>> {
    match values {
        None => Ok(None),
        Some([x, y, w, h]) => {
            if *w <= 0 || *h <= 0 {
                bail!("viewport width and height must be positive");
            }
            Ok(Some(Rect::new(*x, *y, *w, *h)))
        }
        Some(_) => bail!("viewport expects exactly four values: X Y W H"),
    }
}

/// Builds the textual object-layer dump (mirrors the Java `DumpFile` fields).
fn objects_text(jn: &JnFile) -> String {
    use std::fmt::Write;

    let mut out = String::new();
    let objects = jn.objects();
    let _ = writeln!(out, "Object layer ({}):", objects.len());
    let _ = writeln!(
        out,
        "+-------+------+-------+-------+---------+---------+---------+-------+"
    );
    let _ = writeln!(
        out,
        "| Index | Type |   X   |   Y   | X speed | Y speed | Counter | Info1 |"
    );
    let _ = writeln!(
        out,
        "+-------+------+-------+-------+---------+---------+---------+-------+"
    );
    for object in objects {
        let _ = writeln!(
            out,
            "| {:5} | {:4} | {:5} | {:5} | {:7} | {:7} | {:7} | {:5} |",
            object.index(),
            object.object_type(),
            object.x(),
            object.y(),
            object.x_speed(),
            object.y_speed(),
            object.counter(),
            object.info1()
        );
    }
    let _ = writeln!(
        out,
        "+-------+------+-------+-------+---------+---------+---------+-------+"
    );

    let save = jn.save_data();
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "Save data: level {}, health {}, score {}",
        save.level(),
        save.health(),
        save.score()
    );
    out
}

// ---------------------------------------------------------------------------
// `openjill jn view` - read-only egui level viewer (ported from openjill-jn-view).
// Gated on `editor-ui`.
// ---------------------------------------------------------------------------

#[cfg(feature = "editor-ui")]
mod view {
    use super::{SHARED_DMA_FILE, episode_for};
    use std::path::{Path, PathBuf};

    use anyhow::Result;
    use clap::Args;
    use egui::load::SizedTexture;
    use image::RgbaImage;
    use openjill_core::Palette;
    use openjill_data::dma::DmaFile;
    use openjill_data::jn::JnFile;
    use openjill_data::sha::ShaFile;
    use openjill_export::jn::map_to_png;
    use openjill_ui::widgets::{FileTree, FileTreeState};

    /// Arguments for `openjill jn view`.
    #[derive(Args, Debug)]
    pub struct ViewArgs {
        /// JN map file to open on startup.
        #[arg(short, long)]
        file: Option<PathBuf>,
        /// Directory shown in the file browser (defaults to the file's parent
        /// or `OPENJILL_DATA_DIR`, then `data/original/JILL1`).
        #[arg(short, long)]
        data_dir: Option<PathBuf>,
    }

    /// Launches the read-only JN level viewer GUI.
    pub fn view_command(args: ViewArgs) -> Result<()> {
        let data_dir = resolve_data_dir(args.data_dir.as_deref(), args.file.as_deref());
        let initial_file = args.file.clone();

        let options = eframe::NativeOptions::default();
        eframe::run_native(
            "openjill jn view",
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

    /// Resolves the file-browser root from the flags and environment.
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

    /// Read-only JN level viewer application state.
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
            let file_tree_state =
                FileTreeState::new(data_dir).with_extensions(&["jn1", "jn2", "jn3"]);
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
                                egui::Grid::new("jn_object_inspector").num_columns(2).show(
                                    ui,
                                    |ui| {
                                        inspector_row(ui, "type", object.object_type());
                                        inspector_row(ui, "x", object.x());
                                        inspector_row(ui, "y", object.y());
                                        inspector_row(ui, "x_speed", object.x_speed());
                                        inspector_row(ui, "y_speed", object.y_speed());
                                        inspector_row(ui, "counter", object.counter());
                                        inspector_row(ui, "info1", object.info1());
                                    },
                                );
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

    /// Draws object markers over the map image, highlighting the selected one.
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
        let sha = ShaFile::from_bytes(
            std::fs::read(data_dir.join(episode.sha)).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let dma = DmaFile::from_bytes(
            std::fs::read(data_dir.join(SHARED_DMA_FILE)).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let palette = Palette::jill_vga();
        let image = map_to_png(&jn, &sha, &dma, &palette);
        Ok((jn, image))
    }

    #[cfg(test)]
    mod ui_tests {
        use super::JnViewApp;
        use std::path::{Path, PathBuf};

        /// Resolves the test data directory from `OPENJILL_DATA_DIR` or the
        /// workspace-relative default, returning `None` when neither exists so
        /// the GUI runtime tests self-skip without the copyrighted bytes.
        fn data_dir() -> Option<PathBuf> {
            if let Some(path) = std::env::var_os("OPENJILL_DATA_DIR") {
                return Some(PathBuf::from(path));
            }
            let default = Path::new(env!("CARGO_WORKSPACE_DIR")).join("data/original/JILL1");
            default.is_dir().then_some(default)
        }

        /// Unit under test: `JnViewApp` loading + rendering a real `1.JN1` map.
        ///
        /// Invariant: `new` defers loading to `ui`, and `load_file` against a
        /// headless `egui::Context` reads the JN, resolves the sibling SHA/DMA,
        /// renders the map, uploads the texture, and reports success. Exercises
        /// the GUI viewer's full render path (incl. `ctx.load_texture`) without a
        /// window. Skips without data.
        #[test]
        fn jn_view_app_loads_and_renders_real_map() {
            let Some(dir) = data_dir() else {
                eprintln!("skipping JnViewApp runtime test; data directory missing");
                return;
            };
            let map = dir.join("1.JN1");
            let mut app = JnViewApp::new(&dir, Some(map.clone()));
            assert!(app.loaded.is_none(), "new() must defer loading to ui()");
            let ctx = egui::Context::default();
            app.load_file(&ctx, &map);
            assert!(app.loaded.is_some(), "JnViewApp must load+render 1.JN1");
            assert!(
                app.status.starts_with("Loaded"),
                "status must report success, got: {}",
                app.status
            );
        }

        /// Unit under test: `JnViewApp::load_file` on a missing path.
        ///
        /// Invariant: a failed load clears `loaded` and reports the error in the
        /// status line rather than panicking. Needs no data.
        #[test]
        fn jn_view_app_reports_missing_file() {
            let ctx = egui::Context::default();
            let mut app = JnViewApp::new(Path::new("."), None);
            app.load_file(&ctx, Path::new("does-not-exist.JN1"));
            assert!(app.loaded.is_none(), "missing file must not load");
            assert!(
                app.status.starts_with("Failed to load"),
                "status must report failure, got: {}",
                app.status
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Rect, default_png_path, parse_viewport};
    use std::path::Path;

    #[test]
    fn default_png_path_appends_png_to_file_name() {
        assert_eq!(
            default_png_path(Path::new("/data/1.JN1")),
            Path::new("1.JN1.png")
        );
    }

    #[test]
    fn parse_viewport_accepts_four_positive_values() {
        let rect = parse_viewport(Some(&[16, 32, 64, 48])).expect("valid viewport");
        assert_eq!(rect, Some(Rect::new(16, 32, 64, 48)));
        assert_eq!(parse_viewport(None).expect("none ok"), None);
    }

    #[test]
    fn parse_viewport_rejects_nonpositive_size() {
        assert!(parse_viewport(Some(&[0, 0, 0, 10])).is_err());
        assert!(parse_viewport(Some(&[0, 0, 10, -1])).is_err());
    }
}
