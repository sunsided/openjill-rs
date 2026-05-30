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
