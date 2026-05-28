//! `openjill-dma-extract` — CLI that dumps `JILL.DMA` tile metadata.
//!
//! Ports the Java `dma-file-extractor` on top of the `openjill-data` parser
//! and the `openjill-export::dma` formatters. Emits a human-readable text
//! table (default), CSV, or JSON.

#![forbid(unsafe_code)]

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use openjill_data::dma::DmaFile;
use openjill_export::dma::{table_to_csv, table_to_text};

/// Output format for the DMA dump.
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum Format {
    /// Aligned human-readable text table (default).
    Text,
    /// Comma-separated values.
    Csv,
    /// JSON array of entry objects.
    Json,
}

/// Dumps the entries of a Jill `JILL.DMA` tile-metadata file.
#[derive(Debug, Parser)]
#[command(name = "openjill-dma-extract", version, about)]
struct Cli {
    /// DMA file to read.
    #[arg(short, long)]
    file: PathBuf,
    /// Output file (defaults to stdout).
    #[arg(short, long)]
    out: Option<PathBuf>,
    /// Emit CSV output.
    #[arg(long, conflicts_with = "json")]
    csv: bool,
    /// Emit JSON output.
    #[arg(long)]
    json: bool,
}

impl Cli {
    /// Resolves the effective output format from the flags.
    fn format(&self) -> Format {
        if self.json {
            Format::Json
        } else if self.csv {
            Format::Csv
        } else {
            Format::Text
        }
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let bytes = std::fs::read(&cli.file)
        .with_context(|| format!("failed to read DMA file {}", cli.file.display()))?;
    let dma = DmaFile::from_bytes(bytes)
        .with_context(|| format!("failed to parse DMA file {}", cli.file.display()))?;

    let rendered = render(&dma, cli.format());

    match &cli.out {
        Some(path) => std::fs::write(path, rendered)
            .with_context(|| format!("failed to write {}", path.display()))?,
        None => print!("{rendered}"),
    }
    Ok(())
}

/// Renders the DMA file in the requested format.
fn render(dma: &DmaFile, format: Format) -> String {
    match format {
        Format::Text => table_to_text(dma),
        Format::Csv => table_to_csv(dma),
        Format::Json => entries_to_json(dma),
    }
}

/// Serialises the DMA entries to a pretty-printed JSON array.
///
/// Each element carries the numeric `map_code`, `tileset`, `tile`, and `flags`
/// fields, keeping the dump round-trippable.
fn entries_to_json(dma: &DmaFile) -> String {
    let entries: Vec<serde_json::Value> = dma
        .entries()
        .iter()
        .map(|entry| {
            serde_json::json!({
                "map_code": entry.map_code(),
                "tileset": entry.tileset(),
                "tile": entry.tile(),
                "flags": entry.flags(),
            })
        })
        .collect();
    let mut text = serde_json::to_string_pretty(&entries).unwrap_or_else(|_| "[]".to_string());
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::{Cli, Format};
    use clap::Parser;

    fn parse(args: &[&str]) -> Cli {
        Cli::try_parse_from(args).expect("args should parse")
    }

    #[test]
    fn format_defaults_to_text() {
        let cli = parse(&["dma", "--file", "x.dma"]);
        assert_eq!(cli.format(), Format::Text);
    }

    #[test]
    fn csv_and_json_flags_select_format() {
        assert_eq!(parse(&["dma", "-f", "x", "--csv"]).format(), Format::Csv);
        assert_eq!(parse(&["dma", "-f", "x", "--json"]).format(), Format::Json);
    }

    #[test]
    fn csv_and_json_conflict() {
        assert!(Cli::try_parse_from(["dma", "-f", "x", "--csv", "--json"]).is_err());
    }
}
