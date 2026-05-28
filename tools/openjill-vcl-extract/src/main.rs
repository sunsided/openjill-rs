//! `openjill-vcl-extract` — CLI that dumps `*.VCL` text entries.
//!
//! Built on the `openjill-data` parser and the `openjill-export::vcl`
//! formatters. Dumps all text entries (text or JSON) or a single entry.

#![forbid(unsafe_code)]

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use openjill_data::vcl::VclFile;
use openjill_export::vcl::{entries_to_json, entries_to_text, escape_text_payload};

/// Dumps the text entries of a Jill `*.VCL` file.
#[derive(Debug, Parser)]
#[command(name = "openjill-vcl-extract", version, about)]
struct Cli {
    /// VCL file to read.
    #[arg(short, long)]
    file: PathBuf,
    /// Output file (defaults to stdout).
    #[arg(short, long)]
    out: Option<PathBuf>,
    /// Emit JSON instead of plain text.
    #[arg(long)]
    json: bool,
    /// Dump only a single entry by index.
    #[arg(long)]
    entry: Option<usize>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let bytes = std::fs::read(&cli.file)
        .with_context(|| format!("failed to read VCL file {}", cli.file.display()))?;
    let vcl = VclFile::from_bytes(bytes)
        .with_context(|| format!("failed to parse VCL file {}", cli.file.display()))?;

    let rendered = match cli.entry {
        Some(index) => render_single(&vcl, index, cli.json)?,
        None => render_all(&vcl, cli.json),
    };

    match &cli.out {
        Some(path) => std::fs::write(path, rendered)
            .with_context(|| format!("failed to write {}", path.display()))?,
        None => print!("{rendered}"),
    }
    Ok(())
}

/// Renders every entry as text or JSON.
fn render_all(vcl: &VclFile, json: bool) -> String {
    if json {
        entries_to_json(vcl)
    } else {
        entries_to_text(vcl)
    }
}

/// Renders one entry selected by its logical index (the same index shown in
/// the full text dump), erroring when no entry carries that index.
fn render_single(vcl: &VclFile, index: usize, json: bool) -> Result<String> {
    let entry = vcl
        .text_entries()
        .iter()
        .find(|entry| entry.index() == index)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "no entry with index {index} (file has {} entries)",
                vcl.text_entry_count()
            )
        })?;
    if json {
        // Mirror the `entries_to_json` schema (index + payload) for a single
        // entry so single/full JSON dumps stay consistent.
        let value = serde_json::json!({
            "index": entry.index(),
            "payload": entry.text(),
        });
        let mut text = serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".to_string());
        text.push('\n');
        Ok(text)
    } else {
        let mut text = escape_text_payload(entry.text());
        text.push('\n');
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::Cli;
    use clap::Parser;

    #[test]
    fn parses_entry_and_json_flags() {
        let cli = Cli::try_parse_from(["vcl", "-f", "x.vcl", "--entry", "3", "--json"])
            .expect("args parse");
        assert_eq!(cli.entry, Some(3));
        assert!(cli.json);
    }

    #[test]
    fn entry_defaults_to_none() {
        let cli = Cli::try_parse_from(["vcl", "-f", "x.vcl"]).expect("args parse");
        assert_eq!(cli.entry, None);
        assert!(!cli.json);
    }
}
