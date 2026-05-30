//! `openjill cfg` subcommands: dump `*.CFG` high scores / save slots, and a
//! read-only GUI inspector.
//!
//! Ports the former `openjill-cfg-extract` (text/JSON dump) and
//! `openjill-cfg-view` (egui inspector) tools onto the `openjill-data` parser
//! and the `openjill-export::cfg` formatters. Save-slot file names embed the
//! episode JN prefix, so the episode must be supplied (defaults to `JN1`).

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Args, Subcommand, ValueEnum};
use openjill_data::cfg::CfgFile;
use openjill_export::cfg::{save_slots_to_text, scores_to_text};
#[cfg(feature = "editor-ui")]
use view::ViewArgs;

/// Episode JN prefix used for save-slot file names.
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum EpisodePrefix {
    /// Episode 1 (`JN1`).
    Jn1,
    /// Episode 2 (`JN2`).
    Jn2,
    /// Episode 3 (`JN3`).
    Jn3,
}

impl EpisodePrefix {
    /// All prefixes, for the GUI episode picker.
    #[cfg(feature = "editor-ui")]
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

/// Actions for the `openjill cfg` subcommand.
#[derive(Debug, Subcommand)]
pub enum Action {
    /// Dump the high-score table and/or save slots (text or JSON).
    Extract(ExtractArgs),
    /// Open the read-only CFG inspector GUI.
    #[cfg(feature = "editor-ui")]
    View(ViewArgs),
}

/// Arguments for `openjill cfg extract`.
#[derive(Args, Debug)]
pub struct ExtractArgs {
    /// CFG file to read.
    #[arg(short, long)]
    file: PathBuf,
    /// Episode JN prefix for save-slot file names.
    #[arg(short, long, value_enum, default_value_t = EpisodePrefix::Jn1)]
    episode: EpisodePrefix,
    /// Output file (defaults to stdout).
    #[arg(short, long)]
    out: Option<PathBuf>,
    /// Dump only the high-score table.
    #[arg(long)]
    scores: bool,
    /// Dump only the save slots.
    #[arg(long)]
    saves: bool,
    /// Emit JSON instead of text.
    #[arg(long)]
    json: bool,
}

impl ExtractArgs {
    /// Whether the high-score table should be included.
    fn want_scores(&self) -> bool {
        self.scores || !self.saves
    }

    /// Whether the save slots should be included.
    fn want_saves(&self) -> bool {
        self.saves || !self.scores
    }
}

/// Runs `openjill cfg <action>`.
pub fn run(action: Action) -> Result<()> {
    match action {
        Action::Extract(args) => extract_command(args),
        #[cfg(feature = "editor-ui")]
        Action::View(args) => view::view_command(args),
    }
}

/// Dumps the high-score table and/or save slots of a `*.CFG` file.
fn extract_command(args: ExtractArgs) -> Result<()> {
    let bytes = std::fs::read(&args.file)
        .with_context(|| format!("failed to read CFG file {}", args.file.display()))?;
    let cfg = CfgFile::from_bytes(bytes, args.episode.as_str())
        .with_context(|| format!("failed to parse CFG file {}", args.file.display()))?;

    let rendered = if args.json {
        render_json(&cfg, args.want_scores(), args.want_saves())
    } else {
        render_text(
            &cfg,
            args.episode.as_str(),
            args.want_scores(),
            args.want_saves(),
        )
    };

    match &args.out {
        Some(path) => std::fs::write(path, rendered)
            .with_context(|| format!("failed to write {}", path.display()))?,
        None => print!("{rendered}"),
    }
    Ok(())
}

/// Renders the requested sections as text, scores then saves.
fn render_text(cfg: &CfgFile, jn_ext: &str, want_scores: bool, want_saves: bool) -> String {
    let mut out = String::new();
    if want_scores {
        out.push_str(&scores_to_text(cfg));
    }
    if want_saves {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&save_slots_to_text(cfg, jn_ext));
    }
    out
}

/// Renders the requested sections as a JSON object.
fn render_json(cfg: &CfgFile, want_scores: bool, want_saves: bool) -> String {
    let mut root = serde_json::Map::new();
    if want_scores {
        let scores: Vec<serde_json::Value> = cfg
            .high_scores()
            .iter()
            .map(|hs| serde_json::json!({ "name": hs.name(), "score": hs.score() }))
            .collect();
        root.insert("high_scores".to_string(), serde_json::Value::Array(scores));
    }
    if want_saves {
        let saves: Vec<serde_json::Value> = cfg
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
        root.insert("save_slots".to_string(), serde_json::Value::Array(saves));
    }
    let mut text = serde_json::to_string_pretty(&serde_json::Value::Object(root))
        .unwrap_or_else(|_| "{}".to_string());
    text.push('\n');
    text
}

// ---------------------------------------------------------------------------
// `openjill cfg view` - read-only egui inspector (ported from openjill-cfg-view).
// Gated on `editor-ui`.
// ---------------------------------------------------------------------------

#[cfg(feature = "editor-ui")]
mod view {
    use super::EpisodePrefix;
    use std::path::{Path, PathBuf};

    use anyhow::Result;
    use clap::Args;
    use openjill_data::cfg::CfgFile;
    use openjill_export::cfg::{save_slots_to_text, scores_to_text};
    use openjill_ui::widgets::{FileTree, FileTreeState};

    /// Arguments for `openjill cfg view`.
    #[derive(Args, Debug)]
    pub struct ViewArgs {
        /// CFG file to open on startup.
        #[arg(short, long)]
        file: Option<PathBuf>,
        /// Directory shown in the file browser (defaults to the file's parent
        /// or `OPENJILL_DATA_DIR`, then `data/original/JILL1`).
        #[arg(short, long)]
        data_dir: Option<PathBuf>,
    }

    /// Which table tab is shown.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum Tab {
        Scores,
        Saves,
    }

    /// Launches the read-only CFG inspector GUI.
    pub fn view_command(args: ViewArgs) -> Result<()> {
        let data_dir = resolve_data_dir(args.data_dir.as_deref(), args.file.as_deref());
        let initial_file = args.file.clone();

        let options = eframe::NativeOptions::default();
        eframe::run_native(
            "openjill cfg view",
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

    /// A loaded CFG together with its source path.
    struct LoadedCfg {
        path: PathBuf,
        cfg: CfgFile,
    }

    /// Read-only CFG inspector application state.
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

        /// Exports the loaded CFG to a sibling file with the given extension.
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
}

#[cfg(test)]
mod tests {
    use super::EpisodePrefix;

    #[test]
    fn episode_prefix_strings() {
        assert_eq!(EpisodePrefix::Jn1.as_str(), "JN1");
        assert_eq!(EpisodePrefix::Jn2.as_str(), "JN2");
        assert_eq!(EpisodePrefix::Jn3.as_str(), "JN3");
    }
}
