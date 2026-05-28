//! `openjill-jn-extract` — CLI that renders `*.JN?` maps to PNG and dumps
//! their object layer.
//!
//! Ports the Java `jn-file-extractor` (`DrawFile` + `DumpFile`) on top of the
//! `openjill-data` parser and the `openjill-export::jn` renderer. Rendering a
//! map needs the episode SHA tileset and the shared `JILL.DMA` tile-metadata
//! file; both are resolved next to the JN file unless overridden.

#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::Parser;
use openjill_core::Palette;
use openjill_core::entity::Rect;
use openjill_data::dma::DmaFile;
use openjill_data::episode::{self, Episode};
use openjill_data::jn::JnFile;
use openjill_data::sha::ShaFile;
use openjill_export::jn::map_to_png_with_viewport;

/// Shared tile-metadata file name (identical across episodes).
const SHARED_DMA_FILE: &str = "JILL.DMA";

/// Renders a Jill `*.JN?` map to PNG or dumps its object layer.
#[derive(Debug, Parser)]
#[command(name = "openjill-jn-extract", version, about)]
struct Cli {
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

fn main() -> Result<()> {
    let cli = Cli::parse();
    let jn_bytes = std::fs::read(&cli.file)
        .with_context(|| format!("failed to read JN file {}", cli.file.display()))?;
    let jn = JnFile::from_bytes(jn_bytes)
        .with_context(|| format!("failed to parse JN file {}", cli.file.display()))?;

    if cli.objects {
        print!("{}", objects_text(&jn));
        return Ok(());
    }

    let data_dir = cli.file.parent().unwrap_or_else(|| Path::new("."));
    let episode = episode_for(&cli.file);
    let sha_path = cli
        .sha
        .clone()
        .unwrap_or_else(|| data_dir.join(episode.sha));
    let dma_path = cli
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

    let viewport = parse_viewport(cli.viewport.as_deref())?;
    let palette = Palette::jill_vga();
    let image = map_to_png_with_viewport(&jn, &sha, &dma, &palette, viewport);
    if image.width() == 0 || image.height() == 0 {
        bail!("viewport does not intersect the map; nothing to render");
    }

    let out_path = cli
        .out
        .clone()
        .unwrap_or_else(|| default_png_path(&cli.file));
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
