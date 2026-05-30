//! `openjill sha` subcommands: render `*.SHA` tilesets to PNG (and dump tables).
//!
//! Ports the former `openjill-sha-extract` tool onto the `openjill-data` parser
//! and the `openjill-export::sha` renderer. Each surviving tileset is written as
//! one atlas PNG (`tileset_<index>.png`).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use openjill_core::JILL_VGA_PALETTE;
use openjill_data::sha::{ShaFile, ShaTileSet};
use openjill_export::sha::{ScreenMode, TileFilter, TilesetColorOutput, tileset_to_png};

/// Actions for the `openjill sha` subcommand.
#[derive(Debug, Subcommand)]
pub enum Action {
    /// Render tilesets to PNG (or print the header/tileset tables with `--dump`).
    Extract(ExtractArgs),
    /// Open the read-only tileset viewer GUI.
    #[cfg(feature = "editor-ui")]
    Edit(EditArgs),
}

/// Arguments for `openjill sha extract`.
#[derive(Args, Debug)]
pub struct ExtractArgs {
    /// SHA file to read.
    #[arg(short, long)]
    file: PathBuf,
    /// Output directory for the PNG files (created if missing; defaults to `.`).
    #[arg(short, long)]
    out: Option<PathBuf>,
    /// Print the SHA header / tileset tables instead of extracting.
    #[arg(short, long)]
    dump: bool,
    /// Only include tilesets renderable in CGA mode (skips 8-bit tilesets).
    #[arg(short, long)]
    cga: bool,
    /// Only include tilesets renderable in EGA mode (skips 8-bit tilesets).
    #[arg(short, long)]
    ega: bool,
    /// Include all tilesets (VGA mode, the default).
    #[arg(short, long)]
    vga: bool,
    /// Extract only font tilesets.
    #[arg(short = 't', long)]
    fontonly: bool,
    /// Extract only picture (non-font) tilesets.
    #[arg(short = 'p', long)]
    pictureonly: bool,
}

/// Runs `openjill sha <action>`.
pub fn run(action: Action) -> Result<()> {
    match action {
        Action::Extract(args) => extract_command(args),
        #[cfg(feature = "editor-ui")]
        Action::Edit(args) => edit_command(args),
    }
}

/// Renders the SHA tilesets to PNG, or prints the tables when `--dump` is set.
fn extract_command(args: ExtractArgs) -> Result<()> {
    let bytes = std::fs::read(&args.file)
        .with_context(|| format!("failed to read SHA file {}", args.file.display()))?;
    let sha = ShaFile::from_bytes(bytes)
        .with_context(|| format!("failed to parse SHA file {}", args.file.display()))?;

    if args.dump {
        print!("{}", dump_text(&sha));
        return Ok(());
    }

    let out_dir = args.out.as_deref().unwrap_or_else(|| Path::new("."));
    let mode = resolve_mode(args.cga, args.ega, args.vga);
    let filter = resolve_filter(args.fontonly, args.pictureonly);
    let written = extract(&sha, out_dir, mode, filter)?;
    println!(
        "Wrote {written} tileset PNG(s) to {} (mode {mode:?}, fonts={}, pictures={})",
        out_dir.display(),
        filter.fonts,
        filter.pictures
    );
    Ok(())
}

/// Renders every tileset that passes the mode and kind filters into
/// `tileset_<index>.png` under `out_dir`, returning the number written.
fn extract(sha: &ShaFile, out_dir: &Path, mode: ScreenMode, filter: TileFilter) -> Result<usize> {
    std::fs::create_dir_all(out_dir)
        .with_context(|| format!("failed to create output directory {}", out_dir.display()))?;
    let output = TilesetColorOutput::Colored {
        palette: Arc::new(JILL_VGA_PALETTE),
    };

    let mut written = 0;
    for tileset in sha.tilesets() {
        if !tileset_matches_mode(tileset, mode) || !tileset_matches_filter(tileset, filter) {
            continue;
        }
        let image = tileset_to_png(tileset, output.clone());
        let path = out_dir.join(format!("tileset_{}.png", tileset.entry_index()));
        image
            .save(&path)
            .with_context(|| format!("failed to write {}", path.display()))?;
        written += 1;
    }

    if written == 0 {
        bail!("no tilesets matched the requested mode/kind filters");
    }
    Ok(written)
}

/// Selects the screen mode from the CGA/EGA/VGA flags.
///
/// Any explicit flag overrides the VGA default, with CGA taking precedence over
/// EGA over VGA when several are passed. CGA and EGA both restrict the export to
/// sub-8-bit tilesets.
fn resolve_mode(cga: bool, ega: bool, _vga: bool) -> ScreenMode {
    if cga {
        ScreenMode::Cga
    } else if ega {
        ScreenMode::Ega
    } else {
        ScreenMode::Vga
    }
}

/// Selects the tile-kind filter from the `--fontonly` / `--pictureonly` flags.
///
/// With neither flag both kinds are exported; a single flag restricts to that
/// kind; passing both is equivalent to exporting both.
fn resolve_filter(fontonly: bool, pictureonly: bool) -> TileFilter {
    match (fontonly, pictureonly) {
        (true, false) => TileFilter {
            fonts: true,
            pictures: false,
        },
        (false, true) => TileFilter {
            fonts: false,
            pictures: true,
        },
        _ => TileFilter {
            fonts: true,
            pictures: true,
        },
    }
}

/// Returns `true` when `tileset` should be included for the given screen mode.
///
/// VGA accepts every tileset; CGA and EGA skip 8-bit (VGA-exclusive) tilesets.
fn tileset_matches_mode(tileset: &ShaTileSet, mode: ScreenMode) -> bool {
    match mode {
        ScreenMode::Vga => true,
        ScreenMode::Cga | ScreenMode::Ega => tileset.bit_depth() < 8,
    }
}

/// Returns `true` when `tileset` should be included given the tile-kind filter.
fn tileset_matches_filter(tileset: &ShaTileSet, filter: TileFilter) -> bool {
    if tileset.is_font() {
        filter.fonts
    } else {
        filter.pictures
    }
}

/// Builds the textual header / tileset dump printed by `--dump`.
fn dump_text(sha: &ShaFile) -> String {
    use std::fmt::Write;

    let mut out = String::new();
    let header = sha.header();

    let _ = writeln!(out, "Header entries:");
    let _ = writeln!(out, "+-------+-----------------+--------------+----------+");
    let _ = writeln!(out, "| Index | Tileset offset  | Tileset size | is valid |");
    let _ = writeln!(out, "+-------+-----------------+--------------+----------+");
    for entry in header.entries() {
        let _ = writeln!(
            out,
            "| {:5} |     {:08X}    |     {:04X}     | {:8} |",
            entry.index(),
            entry.offset(),
            entry.size(),
            entry.is_valid()
        );
    }
    let _ = writeln!(out, "+-------+-----------------+--------------+----------+");

    let _ = writeln!(out);
    let _ = writeln!(out, "Tilesets:");
    let _ = writeln!(
        out,
        "+-------+----------+------+---------+----------+----------+----------+-----------+-------+---------+"
    );
    let _ = writeln!(
        out,
        "| Index |  Offset  | Size | Nb tile | Cga size | Ega size | Vga size | Bit depth | Font  | Picture |"
    );
    let _ = writeln!(
        out,
        "+-------+----------+------+---------+----------+----------+----------+-----------+-------+---------+"
    );
    for tileset in sha.tilesets() {
        let _ = writeln!(
            out,
            "| {:5} | {:08X} | {:04X} | {:7} | {:8} | {:8} | {:8} | {:9} | {:5} | {:7} |",
            tileset.entry_index(),
            tileset.offset(),
            tileset.size(),
            tileset.tile_count(),
            tileset.cga_size(),
            tileset.ega_size(),
            tileset.vga_size(),
            tileset.bit_depth(),
            tileset.is_font(),
            !tileset.is_font()
        );
    }
    let _ = writeln!(
        out,
        "+-------+----------+------+---------+----------+----------+----------+-----------+-------+---------+"
    );
    out
}

// ---------------------------------------------------------------------------
// `openjill sha edit` - read-only egui tileset viewer (ported from the former
// openjill-sha-edit tool). Gated on `editor-ui` so the CLI-only `editor` build
// does not pull eframe/egui/openjill-ui.
// ---------------------------------------------------------------------------

#[cfg(feature = "editor-ui")]
pub use edit::{EditArgs, edit_command};

#[cfg(feature = "editor-ui")]
mod edit {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use anyhow::Result;
    use clap::Args;
    use openjill_core::{JILL_VGA_PALETTE, Palette};
    use openjill_data::sha::ShaFile;
    use openjill_export::sha::{TilesetColorOutput, tileset_to_png};
    use openjill_ui::widgets::{
        FileTree, FileTreeState, PaletteFilter, PalettePicker, TileGrid, TileGridTexture,
    };

    /// Arguments for `openjill sha edit`.
    #[derive(Args, Debug)]
    pub struct EditArgs {
        /// SHA file to open on startup.
        #[arg(short, long)]
        file: Option<PathBuf>,
        /// Directory shown in the file browser (defaults to the file's parent
        /// or `OPENJILL_DATA_DIR`, then `data/original/JILL1`).
        #[arg(short, long)]
        data_dir: Option<PathBuf>,
    }

    /// Launches the read-only SHA tileset viewer GUI.
    pub fn edit_command(args: EditArgs) -> Result<()> {
        let data_dir = resolve_data_dir(args.data_dir.as_deref(), args.file.as_deref());
        let initial_file = args.file.clone();

        let options = eframe::NativeOptions::default();
        eframe::run_native(
            "openjill sha edit",
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

    /// A parsed SHA file together with its source path.
    struct LoadedSha {
        path: PathBuf,
        sha: ShaFile,
    }

    /// Read-only SHA tileset viewer application state.
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

        /// Ensures `tile_texture` matches the selected tileset, building it from
        /// the wgpu render state when needed.
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

    #[cfg(test)]
    mod ui_tests {
        use super::ShaEditApp;
        use std::path::{Path, PathBuf};

        /// Returns the test data directory only when it exists and contains
        /// every `required` file, resolving it from `OPENJILL_DATA_DIR` or the
        /// workspace-relative default. Returns `None` (so the GUI runtime tests
        /// self-skip) when the directory is missing or the install is partial -
        /// including when `OPENJILL_DATA_DIR` points at a wrong directory -
        /// rather than failing on absent or misconfigured data.
        fn data_dir_with(required: &[&str]) -> Option<PathBuf> {
            let dir = match std::env::var_os("OPENJILL_DATA_DIR") {
                Some(path) => PathBuf::from(path),
                None => Path::new(env!("CARGO_WORKSPACE_DIR")).join("data/original/JILL1"),
            };
            (dir.is_dir() && required.iter().all(|name| dir.join(name).is_file())).then_some(dir)
        }

        /// Unit under test: `ShaEditApp` constructed against a real `JILL1.SHA`.
        ///
        /// Invariant: the app loads + parses the tileset on construction without
        /// a window - `loaded` is populated, the first tileset is selected, and
        /// the status line reports success. Guards that the GUI viewer's load
        /// path works end to end against the original data. Skips without data.
        #[test]
        fn sha_edit_app_loads_real_file() {
            let Some(dir) = data_dir_with(&["JILL1.SHA"]) else {
                eprintln!("skipping ShaEditApp runtime test; JILL1.SHA missing");
                return;
            };
            let app = ShaEditApp::new(&dir, Some(dir.join("JILL1.SHA")));
            assert!(app.loaded.is_some(), "ShaEditApp must load JILL1.SHA");
            assert_eq!(app.selected_tileset, Some(0), "first tileset selected");
            assert!(
                app.status.starts_with("Loaded"),
                "status must report success, got: {}",
                app.status
            );
        }

        /// Unit under test: `ShaEditApp::load_file` on a missing path.
        ///
        /// Invariant: a failed load clears `loaded` and reports the error in the
        /// status line rather than panicking. Needs no data.
        #[test]
        fn sha_edit_app_reports_missing_file() {
            let mut app = ShaEditApp::new(Path::new("."), None);
            app.load_file(Path::new("does-not-exist.SHA"));
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
    use super::{ScreenMode, TileFilter, resolve_filter, resolve_mode};

    #[test]
    fn resolve_mode_defaults_to_vga() {
        assert_eq!(resolve_mode(false, false, false), ScreenMode::Vga);
        assert_eq!(resolve_mode(false, false, true), ScreenMode::Vga);
    }

    #[test]
    fn resolve_mode_prefers_cga_then_ega() {
        assert_eq!(resolve_mode(true, false, false), ScreenMode::Cga);
        assert_eq!(resolve_mode(false, true, false), ScreenMode::Ega);
        assert_eq!(resolve_mode(true, true, true), ScreenMode::Cga);
        assert_eq!(resolve_mode(false, true, true), ScreenMode::Ega);
    }

    #[test]
    fn resolve_filter_handles_each_combination() {
        assert_eq!(
            resolve_filter(false, false),
            TileFilter {
                fonts: true,
                pictures: true
            }
        );
        assert_eq!(
            resolve_filter(true, false),
            TileFilter {
                fonts: true,
                pictures: false
            }
        );
        assert_eq!(
            resolve_filter(false, true),
            TileFilter {
                fonts: false,
                pictures: true
            }
        );
        assert_eq!(
            resolve_filter(true, true),
            TileFilter {
                fonts: true,
                pictures: true
            }
        );
    }
}
