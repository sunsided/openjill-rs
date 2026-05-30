//! `openjill vcl` subcommands: dump `*.VCL` text entries and extract sounds.
//!
//! Ports the former `openjill-vcl-extract` tool onto the `openjill-data` parser
//! and the `openjill-export::vcl` formatters: dump all text entries (text or
//! JSON) or one entry, or extract every non-empty sound to a 16-bit WAV.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use openjill_data::vcl::VclFile;
use openjill_export::vcl::{entries_to_json, entries_to_text, escape_text_payload, sound_to_wav};

/// Actions for the `openjill vcl` subcommand.
#[derive(Debug, Subcommand)]
pub enum Action {
    /// Dump text entries (text/JSON), or extract sounds to WAV with `--wav`.
    Extract(ExtractArgs),
}

/// Arguments for `openjill vcl extract`.
#[derive(Args, Debug)]
pub struct ExtractArgs {
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

/// Runs `openjill vcl <action>`.
pub fn run(action: Action) -> Result<()> {
    match action {
        Action::Extract(args) => extract_command(args),
    }
}

/// Dumps text entries (or extracts sounds with `--wav`) from a `*.VCL` file.
fn extract_command(args: ExtractArgs) -> Result<()> {
    let bytes = std::fs::read(&args.file)
        .with_context(|| format!("failed to read VCL file {}", args.file.display()))?;
    let vcl = VclFile::from_bytes(bytes)
        .with_context(|| format!("failed to parse VCL file {}", args.file.display()))?;

    if let Some(dir) = &args.wav {
        return extract_sounds(&vcl, dir);
    }

    let rendered = match args.entry {
        Some(index) => render_single(&vcl, index, args.json)?,
        None => render_all(&vcl, args.json),
    };

    match &args.out {
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

/// Renders one entry selected by its logical index (the same index shown in the
/// full text dump), erroring when no entry carries that index.
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
