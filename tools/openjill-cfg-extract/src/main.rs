//! `openjill-cfg-extract` — CLI that dumps `JILL?.CFG` high scores and save
//! slots.
//!
//! Built on the `openjill-data` parser and the `openjill-export::cfg`
//! formatters. The save-slot file names embed the episode JN prefix, so the
//! episode must be supplied (defaults to `JN1`).

#![forbid(unsafe_code)]

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use openjill_data::cfg::CfgFile;
use openjill_export::cfg::{save_slots_to_text, scores_to_text};

/// Episode JN prefix used for save-slot file names.
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum EpisodePrefix {
    /// Episode 1 (`JN1`).
    Jn1,
    /// Episode 2 (`JN2`).
    Jn2,
    /// Episode 3 (`JN3`).
    Jn3,
}

impl EpisodePrefix {
    /// Returns the uppercase prefix string (`JN1` / `JN2` / `JN3`).
    fn as_str(self) -> &'static str {
        match self {
            Self::Jn1 => "JN1",
            Self::Jn2 => "JN2",
            Self::Jn3 => "JN3",
        }
    }
}

/// Dumps the high-score table and save slots of a Jill `*.CFG` file.
#[derive(Debug, Parser)]
#[command(name = "openjill-cfg-extract", version, about)]
struct Cli {
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

impl Cli {
    /// Whether the high-score table should be included.
    fn want_scores(&self) -> bool {
        self.scores || !self.saves
    }

    /// Whether the save slots should be included.
    fn want_saves(&self) -> bool {
        self.saves || !self.scores
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let bytes = std::fs::read(&cli.file)
        .with_context(|| format!("failed to read CFG file {}", cli.file.display()))?;
    let cfg = CfgFile::from_bytes(bytes, cli.episode.as_str())
        .with_context(|| format!("failed to parse CFG file {}", cli.file.display()))?;

    let rendered = if cli.json {
        render_json(&cfg, cli.want_scores(), cli.want_saves())
    } else {
        render_text(
            &cfg,
            cli.episode.as_str(),
            cli.want_scores(),
            cli.want_saves(),
        )
    };

    match &cli.out {
        Some(path) => std::fs::write(path, rendered)
            .with_context(|| format!("failed to write {}", path.display()))?,
        None => print!("{rendered}"),
    }
    Ok(())
}

/// Renders the requested sections as text, in scores-then-saves order to match
/// the Java dump order.
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

#[cfg(test)]
mod tests {
    use super::{Cli, EpisodePrefix};
    use clap::Parser;

    fn parse(args: &[&str]) -> Cli {
        Cli::try_parse_from(args).expect("args should parse")
    }

    #[test]
    fn defaults_dump_both_sections_for_episode_one() {
        let cli = parse(&["cfg", "-f", "x.cfg"]);
        assert_eq!(cli.episode, EpisodePrefix::Jn1);
        assert!(cli.want_scores() && cli.want_saves());
    }

    #[test]
    fn scores_flag_excludes_saves_and_vice_versa() {
        let scores = parse(&["cfg", "-f", "x", "--scores"]);
        assert!(scores.want_scores() && !scores.want_saves());
        let saves = parse(&["cfg", "-f", "x", "--saves"]);
        assert!(!saves.want_scores() && saves.want_saves());
    }

    #[test]
    fn episode_flag_maps_to_prefix() {
        assert_eq!(
            parse(&["cfg", "-f", "x", "-e", "jn3"]).episode.as_str(),
            "JN3"
        );
    }
}
