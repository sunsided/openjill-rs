//! `openjill-vcl-extract` — CLI that dumps `*.VCL` text entries and sounds.
//!
//! Built on the `openjill-data` parser and the `openjill-export::vcl`
//! formatters. Dumps all text entries (text or JSON) or a single entry, or
//! extracts every non-empty sound to a 16-bit WAV with `--wav <DIR>`.

#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Parser;
use openjill_data::vcl::VclFile;
use openjill_export::vcl::{entries_to_json, entries_to_text, escape_text_payload, sound_to_wav};

/// Dumps the text entries (and optionally the sounds) of a Jill `*.VCL` file.
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
    /// Extract every non-empty sound to a 16-bit WAV in this directory
    /// (`sound-NN.wav`), instead of dumping text.
    #[arg(long, value_name = "DIR")]
    wav: Option<PathBuf>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let bytes = std::fs::read(&cli.file)
        .with_context(|| format!("failed to read VCL file {}", cli.file.display()))?;
    let vcl = VclFile::from_bytes(bytes)
        .with_context(|| format!("failed to parse VCL file {}", cli.file.display()))?;

    if let Some(dir) = &cli.wav {
        return extract_sounds(&vcl, dir);
    }

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

/// Writes every non-empty sound to `dir/sound-NN.wav` and prints a summary.
fn extract_sounds(vcl: &VclFile, dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir)
        .with_context(|| format!("failed to create output directory {}", dir.display()))?;

    let mut count = 0;
    for (index, slot) in vcl.sounds().iter().enumerate() {
        let Some(sound) = slot else {
            continue;
        };
        let path = dir.join(format!("sound-{index:02}.wav"));
        std::fs::write(&path, sound_to_wav(sound))
            .with_context(|| format!("failed to write {}", path.display()))?;
        println!(
            "{} ({} samples @ {} Hz)",
            path.display(),
            sound.pcm().len(),
            sound.frequency()
        );
        count += 1;
    }
    println!("wrote {count} sound(s) to {}", dir.display());
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
        assert_eq!(cli.wav, None);
    }

    #[test]
    fn parses_wav_output_directory() {
        let cli =
            Cli::try_parse_from(["vcl", "-f", "x.vcl", "--wav", "out/sounds"]).expect("args parse");
        assert_eq!(cli.wav, Some(std::path::PathBuf::from("out/sounds")));
    }
}
