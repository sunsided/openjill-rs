//! `openjill-cfg-view` — read-only egui inspector for Jill `*.CFG` files.
//!
//! Shows the high-score table and save slots in a tabbed table view, with an
//! episode picker (the JN prefix the parser needs for save-slot names) and
//! text / JSON export. Read-only.

#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

use anyhow::Result;
use clap::Parser;
use openjill_data::cfg::CfgFile;
use openjill_export::cfg::{save_slots_to_text, scores_to_text};
use openjill_ui::widgets::{FileTree, FileTreeState};

/// Episode JN prefix used for save-slot file names.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EpisodePrefix {
    Jn1,
    Jn2,
    Jn3,
}

impl EpisodePrefix {
    const ALL: [Self; 3] = [Self::Jn1, Self::Jn2, Self::Jn3];

    /// Returns the uppercase prefix string (`JN1` / `JN2` / `JN3`).
    fn as_str(self) -> &'static str {
        match self {
            Self::Jn1 => "JN1",
            Self::Jn2 => "JN2",
            Self::Jn3 => "JN3",
        }
    }
}

/// Which table tab is shown.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Tab {
    Scores,
    Saves,
}

/// Read-only GUI inspector for Jill `*.CFG` files.
#[derive(Debug, Parser)]
#[command(name = "openjill-cfg-view", version, about)]
struct Cli {
    /// CFG file to open on startup.
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
        "openjill-cfg-view",
        options,
        Box::new(move |creation_context| {
            creation_context
                .egui_ctx
                .set_theme(egui::ThemePreference::System);
            Ok(Box::new(CfgViewApp::new(&data_dir, initial_file.clone())))
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

/// A loaded CFG together with its source path.
struct LoadedCfg {
    path: PathBuf,
    cfg: CfgFile,
}

struct CfgViewApp {
    /// File-tree state filtered to `*.CFG`.
    file_tree_state: FileTreeState,
    /// Path most recently selected in the file tree.
    selected_file: Option<PathBuf>,
    /// Currently loaded CFG, if any.
    loaded: Option<LoadedCfg>,
    /// Active table tab.
    tab: Tab,
    /// Episode prefix used to (re)parse save-slot names.
    episode: EpisodePrefix,
    /// Status / error line.
    status: String,
}

impl CfgViewApp {
    /// Builds the app, optionally loading `initial_file` immediately.
    fn new(data_dir: &Path, initial_file: Option<PathBuf>) -> Self {
        let file_tree_state = FileTreeState::new(data_dir).with_extensions(&["cfg"]);
        let mut app = Self {
            file_tree_state,
            selected_file: None,
            loaded: None,
            tab: Tab::Scores,
            episode: EpisodePrefix::Jn1,
            status: String::new(),
        };
        if let Some(path) = initial_file {
            app.load_file(&path);
            app.selected_file = Some(path);
        }
        app
    }

    /// Loads and parses a CFG file with the current episode prefix.
    fn load_file(&mut self, path: &Path) {
        match std::fs::read(path)
            .map_err(|e| e.to_string())
            .and_then(|bytes| {
                CfgFile::from_bytes(bytes, self.episode.as_str()).map_err(|e| e.to_string())
            }) {
            Ok(cfg) => {
                self.status = format!(
                    "Loaded {} ({} scores, {} slots)",
                    path.display(),
                    cfg.high_scores().len(),
                    cfg.save_slots().len()
                );
                self.loaded = Some(LoadedCfg {
                    path: path.to_path_buf(),
                    cfg,
                });
            }
            Err(error) => {
                self.loaded = None;
                self.status = format!("Failed to load {}: {error}", path.display());
            }
        }
    }

    /// Reloads the current file (used after an episode change).
    fn reload(&mut self) {
        if let Some(path) = self.loaded.as_ref().map(|l| l.path.clone()) {
            self.load_file(&path);
        }
    }

    /// Exports the loaded CFG to a sibling file with the given extension and
    /// rendered contents.
    fn export(&mut self, extension: &str, contents: String) {
        let Some(loaded) = &self.loaded else {
            self.status = "No CFG loaded to export".to_string();
            return;
        };
        let out = loaded.path.with_extension(extension);
        match std::fs::write(&out, contents) {
            Ok(()) => self.status = format!("Exported {}", out.display()),
            Err(error) => self.status = format!("Export failed: {error}"),
        }
    }

    /// Renders the loaded CFG as combined text (scores then saves).
    fn export_text(&mut self) {
        let Some(loaded) = &self.loaded else {
            self.status = "No CFG loaded to export".to_string();
            return;
        };
        let mut text = scores_to_text(&loaded.cfg);
        text.push('\n');
        text.push_str(&save_slots_to_text(&loaded.cfg, self.episode.as_str()));
        self.export("txt", text);
    }

    /// Renders the loaded CFG as a JSON object (scores + saves).
    fn export_json(&mut self) {
        let Some(loaded) = &self.loaded else {
            self.status = "No CFG loaded to export".to_string();
            return;
        };
        let scores: Vec<serde_json::Value> = loaded
            .cfg
            .high_scores()
            .iter()
            .map(|hs| serde_json::json!({ "name": hs.name(), "score": hs.score() }))
            .collect();
        let saves: Vec<serde_json::Value> = loaded
            .cfg
            .save_slots()
            .iter()
            .map(|slot| {
                serde_json::json!({
                    "name": slot.name(),
                    "save_game_file": slot.save_game_file(),
                    "save_map_file": slot.save_map_file(),
                })
            })
            .collect();
        let value = serde_json::json!({ "high_scores": scores, "save_slots": saves });
        let text = serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".to_string());
        self.export("json", text);
    }
}

impl eframe::App for CfgViewApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Toolbar.
        egui::Panel::top("cfg_view_toolbar").show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                let previous = self.episode;
                egui::ComboBox::from_label("Episode")
                    .selected_text(self.episode.as_str())
                    .show_ui(ui, |ui| {
                        for option in EpisodePrefix::ALL {
                            ui.selectable_value(&mut self.episode, option, option.as_str());
                        }
                    });
                if self.episode != previous {
                    self.reload();
                }
                if ui.button("Export text").clicked() {
                    self.export_text();
                }
                if ui.button("Export JSON").clicked() {
                    self.export_json();
                }
                ui.separator();
                ui.label(if self.status.is_empty() {
                    "Open a .CFG file from the tree"
                } else {
                    self.status.as_str()
                });
            });
        });

        // Left: file tree filtered to *.CFG.
        egui::Panel::left("cfg_view_files")
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

        // Centre: tabbed table view.
        match &self.loaded {
            Some(loaded) => {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.tab, Tab::Scores, "High scores");
                    ui.selectable_value(&mut self.tab, Tab::Saves, "Save slots");
                });
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| match self.tab {
                    Tab::Scores => scores_table(ui, &loaded.cfg),
                    Tab::Saves => saves_table(ui, &loaded.cfg),
                });
            }
            None => {
                ui.centered_and_justified(|ui| {
                    ui.label("Open a .CFG file from the file tree.");
                });
            }
        }
    }
}

/// Renders the high-score table.
fn scores_table(ui: &mut egui::Ui, cfg: &CfgFile) {
    egui::Grid::new("cfg_scores")
        .num_columns(3)
        .striped(true)
        .show(ui, |ui| {
            ui.strong("Rank");
            ui.strong("Name");
            ui.strong("Score");
            ui.end_row();
            for (rank, hs) in cfg.high_scores().iter().enumerate() {
                ui.label((rank + 1).to_string());
                ui.label(hs.name());
                ui.label(hs.score().to_string());
                ui.end_row();
            }
        });
}

/// Renders the save-slot table.
fn saves_table(ui: &mut egui::Ui, cfg: &CfgFile) {
    egui::Grid::new("cfg_saves")
        .num_columns(4)
        .striped(true)
        .show(ui, |ui| {
            ui.strong("Slot");
            ui.strong("Name");
            ui.strong("Game file");
            ui.strong("Map file");
            ui.end_row();
            for (slot, save) in cfg.save_slots().iter().enumerate() {
                ui.label(slot.to_string());
                ui.label(save.name());
                ui.label(save.save_game_file());
                ui.label(save.save_map_file());
                ui.end_row();
            }
        });
}
