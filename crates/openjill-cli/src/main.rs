#![forbid(unsafe_code)]

use std::ffi::OsStr;
use std::fs;
use std::io::Cursor;
use std::path::{Component, Path, PathBuf};

use anyhow::{Result, bail};
use clap::{Args, Parser, Subcommand, ValueEnum};
use openjill_audio::AudioBackend;
use openjill_core::CoreState;
use openjill_data::ByteReader;
use openjill_data::cfg::CfgFile;
use openjill_data::dma::DmaFile;
use openjill_data::jn::JnFile;
use openjill_data::sha::ShaFile;
use openjill_data::vcl::VclFile;
use openjill_data::{DataDirectory, DataDirectoryError};
use openjill_game::GameApp;
use openjill_render::Renderer;
use png::{BitDepth, ColorType, Encoder};
use sha2::{Digest, Sha256};

/// Environment variable used as a fallback data directory for data utility commands.
const OPENJILL_DATA_DIR_ENV: &str = "OPENJILL_DATA_DIR";
/// Task-runner environment variable used as a data directory override.
const DATA_DIR_ENV: &str = "DATA_DIR";
/// Workspace-relative fallback path used when no flag or environment override is supplied.
const DEFAULT_DATA_DIR: &str = "data/original/JILL1";
/// Default root for generated debug dumps.
const DEFAULT_DUMP_ROOT: &str = "data/extracted/JILL1/debug";
/// Repository-relative directory that must never receive generated output.
const ORIGINAL_DATA_ROOT: &str = "data/original";
/// File name used by `dump sha` for indexed atlas pixels.
const SHA_ATLAS_INDEXED_FILE: &str = "atlas-indexed.png";
/// Pixel padding inserted between packed SHA atlas tiles.
const SHA_ATLAS_TILE_PADDING: usize = 1;
/// Required fixed files for episode 1 verification.
const REQUIRED_FILES: [(&str, ParserDomain); 6] = [
    ("JILL.DMA", ParserDomain::Dma),
    ("JILL1.VCL", ParserDomain::Vcl),
    ("JILL1.CFG", ParserDomain::Cfg),
    ("JILL1.SHA", ParserDomain::Sha),
    ("INTRO.JN1", ParserDomain::Jn),
    ("MAP.JN1", ParserDomain::Jn),
];

#[derive(Debug, Parser)]
#[command(name = "openjill-rs", about = "OpenJill Rust port CLI (stub)")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Run(RunArgs),
    Data {
        #[command(subcommand)]
        command: DataCommand,
    },
    Dump(DumpArgs),
}

#[derive(Debug, Subcommand)]
enum DataCommand {
    Verify(VerifyArgs),
}

#[derive(Args, Debug)]
struct DataDirArgs {
    #[arg(long, default_value = "data/original/jill1")]
    data_dir: PathBuf,
}

#[derive(Args, Debug)]
struct RunArgs {
    #[command(flatten)]
    common: DataDirArgs,
}

#[derive(Args, Debug)]
struct DumpArgs {
    /// Specific dump kind requested by the caller.
    #[command(subcommand)]
    kind: DumpKind,
}

#[derive(Debug, Subcommand)]
enum DumpKind {
    /// Build the DMA dump framework output.
    Dma(DumpCommandArgs),
    /// Build the VCL dump framework output.
    Vcl(DumpCommandArgs),
    /// Build the SHA dump framework output.
    Sha(DumpCommandArgs),
    /// Build the JN dump framework output.
    Jn(DumpCommandArgs),
}

#[derive(Args, Debug)]
struct DumpCommandArgs {
    /// Optional directory containing the original episode data files.
    #[arg(long)]
    data_dir: Option<PathBuf>,
    /// Optional output file or directory for the dump.
    #[arg(long)]
    output: Option<PathBuf>,
    /// Output format.
    #[arg(long, default_value = "json")]
    format: DumpFormat,
    /// Overwrite an existing output path.
    #[arg(long)]
    force: bool,
}

/// Output formats supported by dump commands.
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum DumpFormat {
    /// Structured JSON output.
    Json,
}

#[derive(Args, Debug)]
struct VerifyArgs {
    #[arg(long)]
    data_dir: Option<PathBuf>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    dispatch(cli.command)
}

fn dispatch(command: Command) -> Result<()> {
    match command {
        Command::Run(args) => run_command(args),
        Command::Data {
            command: DataCommand::Verify(args),
        } => data_verify_command(args),
        Command::Dump(args) => dump_command(args),
    }
}

fn run_command(args: RunArgs) -> Result<()> {
    let core = CoreState::new(DataDirectory::new(args.common.data_dir));
    let _game = GameApp::new(core, Renderer::new(), AudioBackend::new());
    println!("'run' command is currently a workspace-foundation stub.");
    Ok(())
}

fn data_verify_command(args: VerifyArgs) -> Result<()> {
    let data_dir = resolve_verify_data_dir(&args);
    let report = verify_data_directory(data_dir)?;
    print_verification_report(&report);
    if report.ok() {
        Ok(())
    } else {
        bail!("{}", report.failure_summary());
    }
}

fn dump_command(args: DumpArgs) -> Result<()> {
    let request = DumpRequest::from_args(args);
    write_dump(request)
}

/// Data-format parser domain used by verification output.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ParserDomain {
    /// `JILL.DMA` parser domain.
    Dma,
    /// `JILL1.VCL` parser domain.
    Vcl,
    /// `JILL1.CFG` parser domain.
    Cfg,
    /// `JILL1.SHA` parser domain.
    Sha,
    /// `*.JN1` parser domain.
    Jn,
}

impl ParserDomain {
    /// Returns the uppercase parser-domain label used in output.
    fn as_str(self) -> &'static str {
        match self {
            Self::Dma => "DMA",
            Self::Vcl => "VCL",
            Self::Cfg => "CFG",
            Self::Sha => "SHA",
            Self::Jn => "JN",
        }
    }

    /// Parses bytes according to the selected domain, returning an error string on failure.
    fn parse(self, bytes: &[u8]) -> std::result::Result<(), String> {
        match self {
            Self::Dma => DmaFile::from_bytes(bytes)
                .map(|_| ())
                .map_err(|error| error.to_string()),
            Self::Vcl => VclFile::from_bytes(bytes)
                .map(|_| ())
                .map_err(|error| error.to_string()),
            Self::Cfg => CfgFile::from_bytes(bytes, "JN1")
                .map(|_| ())
                .map_err(|error| error.to_string()),
            Self::Sha => ShaFile::from_bytes(bytes)
                .map(|_| ())
                .map_err(|error| error.to_string()),
            Self::Jn => JnFile::from_bytes(bytes)
                .map(|_| ())
                .map_err(|error| error.to_string()),
        }
    }
}

/// Per-file verification status.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum VerificationStatus {
    /// File exists and parsed successfully.
    Present,
    /// File could not be resolved in the data directory.
    Missing,
    /// File exists but failed to load or parse.
    Invalid,
}

impl VerificationStatus {
    /// Returns the lowercase status label printed by the command.
    fn as_str(self) -> &'static str {
        match self {
            Self::Present => "present",
            Self::Missing => "missing",
            Self::Invalid => "invalid",
        }
    }
}

/// Verification result for one file entry.
#[derive(Clone, Debug, Eq, PartialEq)]
struct FileVerification {
    /// Requested input file name.
    requested_file: String,
    /// Resolved path relative to the data directory when present.
    resolved_file: Option<PathBuf>,
    /// Parser domain associated with this file.
    parser_domain: ParserDomain,
    /// Verification status for this file.
    status: VerificationStatus,
    /// File size in bytes when present.
    size_bytes: Option<u64>,
    /// Lowercase hexadecimal SHA-256 digest when present.
    checksum_sha256: Option<String>,
    /// Parser or read error text when status is invalid.
    parser_error: Option<String>,
}

/// Full verification report for one data directory.
#[derive(Clone, Debug, Eq, PartialEq)]
struct VerificationReport {
    /// Data directory used for verification.
    data_dir: PathBuf,
    /// Fixed required episode-1 files.
    required_files: Vec<FileVerification>,
    /// Discovered additional `*.JN1` files excluding intro/map.
    jn_files: Vec<FileVerification>,
}

impl VerificationReport {
    /// Returns `true` when every required fixed file is present and valid.
    fn ok(&self) -> bool {
        self.required_files
            .iter()
            .all(|file| file.status == VerificationStatus::Present)
    }

    /// Builds a readable summary that names each missing or invalid required file.
    fn failure_summary(&self) -> String {
        let failures = self
            .required_files
            .iter()
            .filter(|file| file.status != VerificationStatus::Present)
            .map(|file| file.requested_file.as_str())
            .collect::<Vec<_>>();

        if failures.is_empty() {
            "data verification failed".to_string()
        } else {
            format!(
                "data verification failed (missing or invalid required files: {})",
                failures.join(", ")
            )
        }
    }
}

/// Resolves the data directory for `data verify` using CLI flag, environment, and fallback order.
fn resolve_verify_data_dir(args: &VerifyArgs) -> PathBuf {
    resolve_data_dir_with_env(
        args.data_dir.clone(),
        nonempty_env_os(DATA_DIR_ENV).or_else(|| nonempty_env_os(OPENJILL_DATA_DIR_ENV)),
    )
}

/// Resolves the data directory for `dump` using CLI flag, environment, and fallback order.
fn resolve_dump_data_dir(args: &DumpCommandArgs) -> PathBuf {
    resolve_data_dir_with_env(
        args.data_dir.clone(),
        nonempty_env_os(DATA_DIR_ENV).or_else(|| nonempty_env_os(OPENJILL_DATA_DIR_ENV)),
    )
}

/// Reads an environment variable, treating empty values as absent.
fn nonempty_env_os(key: &str) -> Option<std::ffi::OsString> {
    std::env::var_os(key).filter(|value| !value.is_empty())
}

/// Resolves data-directory overrides shared by data utility commands.
fn resolve_data_dir_with_env(
    explicit: Option<PathBuf>,
    env_override: Option<std::ffi::OsString>,
) -> PathBuf {
    if let Some(path) = explicit {
        return path;
    }

    if let Some(path) = env_override {
        return PathBuf::from(path);
    }

    PathBuf::from(DEFAULT_DATA_DIR)
}

/// Supported dump framework kinds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DumpFrameworkKind {
    /// DMA metadata dump framework.
    Dma,
    /// VCL metadata dump framework.
    Vcl,
    /// SHA metadata dump framework.
    Sha,
    /// JN metadata dump framework.
    Jn,
}

impl DumpFrameworkKind {
    /// Returns the lowercase CLI name for this dump kind.
    fn as_str(self) -> &'static str {
        match self {
            Self::Dma => "dma",
            Self::Vcl => "vcl",
            Self::Sha => "sha",
            Self::Jn => "jn",
        }
    }

    /// Returns `true` when this dump writes a directory containing `metadata.json`.
    fn writes_directory(self) -> bool {
        matches!(self, Self::Sha | Self::Jn)
    }

    /// Returns the default repository-relative output path for this dump kind.
    fn default_output(self) -> PathBuf {
        match self {
            Self::Dma => PathBuf::from(DEFAULT_DUMP_ROOT).join("dma.json"),
            Self::Vcl => PathBuf::from(DEFAULT_DUMP_ROOT).join("vcl-text.json"),
            Self::Sha => PathBuf::from(DEFAULT_DUMP_ROOT).join("sha"),
            Self::Jn => PathBuf::from(DEFAULT_DUMP_ROOT).join("jn"),
        }
    }

    /// Returns the required source files for this dump kind.
    fn fixed_source_files(self) -> &'static [&'static str] {
        match self {
            Self::Dma => &["JILL.DMA"],
            Self::Vcl => &["JILL1.VCL"],
            Self::Sha => &["JILL1.SHA"],
            Self::Jn => &["INTRO.JN1", "MAP.JN1"],
        }
    }
}

/// Parsed request for one dump framework command.
#[derive(Debug)]
struct DumpRequest {
    /// Dump kind selected by the caller.
    kind: DumpFrameworkKind,
    /// Data directory resolved from CLI flags, environment, or fallback.
    data_dir: PathBuf,
    /// User-selected output path, or the default path for the kind.
    output: PathBuf,
    /// Output format selected by the caller.
    format: DumpFormat,
    /// Whether existing output may be overwritten.
    force: bool,
}

impl DumpRequest {
    /// Converts clap arguments into a resolved dump request.
    fn from_args(args: DumpArgs) -> Self {
        match args.kind {
            DumpKind::Dma(command) => Self::from_command(DumpFrameworkKind::Dma, command),
            DumpKind::Vcl(command) => Self::from_command(DumpFrameworkKind::Vcl, command),
            DumpKind::Sha(command) => Self::from_command(DumpFrameworkKind::Sha, command),
            DumpKind::Jn(command) => Self::from_command(DumpFrameworkKind::Jn, command),
        }
    }

    /// Converts one dump subcommand into a resolved request.
    fn from_command(kind: DumpFrameworkKind, command: DumpCommandArgs) -> Self {
        let data_dir = resolve_dump_data_dir(&command);
        let output = command.output.unwrap_or_else(|| kind.default_output());
        Self {
            kind,
            data_dir,
            output,
            format: command.format,
            force: command.force,
        }
    }
}

/// Metadata captured for one parsed dump source file.
#[derive(Debug)]
struct DumpSourceFile {
    /// File requested from the data directory.
    requested_file: String,
    /// Case-insensitive path resolved relative to the data directory.
    resolved_file: PathBuf,
    /// Source file size in bytes.
    source_size: usize,
    /// Lowercase hexadecimal SHA-256 digest of the source bytes.
    source_sha256: String,
}

/// Writes the requested dump framework output.
fn write_dump(request: DumpRequest) -> Result<()> {
    match request.format {
        DumpFormat::Json => {}
    }
    let output_file = resolve_dump_output_file(&request)?;
    if request.kind == DumpFrameworkKind::Sha {
        let output_dir = output_file.parent().ok_or_else(|| {
            anyhow::anyhow!("missing output directory for {}", output_file.display())
        })?;
        let atlas_file = output_dir.join(SHA_ATLAS_INDEXED_FILE);
        ensure_output_may_be_written(&output_file, request.force)?;
        ensure_output_may_be_written(&atlas_file, request.force)?;
        let sha_dump = sha_dump_output(&request.data_dir)?;
        write_json_file(&output_file, &sha_dump.metadata_json, request.force)?;
        write_binary_file(&atlas_file, &sha_dump.atlas_indexed_png, request.force)?;
    } else {
        let json = render_dump_json(&request, &output_file)?;
        write_json_file(&output_file, &json, request.force)?;
    }
    println!(
        "wrote {} dump metadata to {}",
        request.kind.as_str(),
        output_file.display()
    );
    Ok(())
}

/// Renders JSON for the selected dump request.
fn render_dump_json(request: &DumpRequest, output_file: &Path) -> Result<String> {
    match request.kind {
        DumpFrameworkKind::Dma => dma_dump_json(&request.data_dir),
        DumpFrameworkKind::Vcl => vcl_dump_json(&request.data_dir),
        DumpFrameworkKind::Jn => jn_dump_json(&request.data_dir),
        DumpFrameworkKind::Sha => {
            unreachable!("SHA dumps are rendered through the dedicated payload writer")
        }
    }
}

/// Parsed source bytes plus metadata for a dump input file.
#[derive(Debug)]
struct DumpInputSource {
    /// Raw source bytes loaded from disk.
    bytes: Vec<u8>,
    /// Source file size in bytes.
    source_size: usize,
    /// Lowercase hexadecimal SHA-256 digest for the source bytes.
    source_sha256: String,
}

/// Loads source bytes through `DataDirectory` and computes metadata used in dumps.
fn read_dump_input_source(
    directory: &DataDirectory,
    requested_file: &str,
) -> Result<DumpInputSource> {
    let resolved_path = directory
        .resolve_path_case_insensitive(requested_file)
        .map_err(|error| anyhow::anyhow!("missing input file {requested_file}: {error}"))?;
    let bytes = fs::read(&resolved_path).map_err(|error| {
        anyhow::anyhow!(
            "failed to read input file {}: {error}",
            resolved_path.display()
        )
    })?;

    let source_size = bytes.len();
    let source_sha256 = sha256_lower_hex(&bytes);

    Ok(DumpInputSource {
        bytes,
        source_size,
        source_sha256,
    })
}

/// Parses one DMA input source and wraps parser failures with file context.
fn parse_dma_source(bytes: Vec<u8>) -> Result<DmaFile> {
    DmaFile::from_bytes(bytes)
        .map_err(|error| anyhow::anyhow!("failed to parse input file JILL.DMA: {error}"))
}

/// Parses one VCL input source and wraps parser failures with file context.
fn parse_vcl_source(bytes: Vec<u8>) -> Result<VclFile> {
    VclFile::from_bytes(bytes)
        .map_err(|error| anyhow::anyhow!("failed to parse input file JILL1.VCL: {error}"))
}

/// Parses one SHA input source and wraps parser failures with file context so
/// callers get actionable diagnostics that name `JILL1.SHA`.
fn parse_sha_source(bytes: Vec<u8>) -> Result<ShaFile> {
    ShaFile::from_bytes(bytes)
        .map_err(|error| anyhow::anyhow!("failed to parse input file JILL1.SHA: {error}"))
}

/// Builds deterministic JSON metadata for a `dump dma` request.
fn dma_dump_json(data_dir: &Path) -> Result<String> {
    let directory = DataDirectory::new(data_dir.to_path_buf());
    let source = read_dump_input_source(&directory, "JILL.DMA")?;
    let DumpInputSource {
        bytes,
        source_size,
        source_sha256,
    } = source;
    let dma = parse_dma_source(bytes)?;

    let entries = dma
        .entries()
        .iter()
        .map(|entry| {
            serde_json::json!({
                "index": entry.index(),
                "source_offset": entry.offset(),
                "map_code": entry.map_code(),
                "map_code_hex": format_u16_hex(entry.map_code()),
                "tile": entry.tile(),
                "tileset": entry.tileset(),
                "flags": entry.flags(),
                "flags_hex": format_u16_hex(entry.flags()),
                "is_msg_touch": entry.is_msg_touch(),
                "is_msg_draw": entry.is_msg_draw(),
                "is_msg_update": entry.is_msg_update(),
                "is_player_thru": entry.is_player_thru(),
                "is_stair": entry.is_stair(),
                "is_vine": entry.is_vine(),
                "name": entry.name(),
            })
        })
        .collect::<Vec<_>>();

    let json = serde_json::json!({
        "source_file": "JILL.DMA",
        "source_size": source_size,
        "source_sha256": source_sha256,
        "entry_count": dma.entry_count(),
        "entries": entries,
    });
    let mut rendered = serde_json::to_string_pretty(&json)
        .map_err(|error| anyhow::anyhow!("failed to serialize dump metadata: {error}"))?;
    rendered.push('\n');
    Ok(rendered)
}

/// Builds deterministic JSON metadata for a `dump vcl` request.
fn vcl_dump_json(data_dir: &Path) -> Result<String> {
    let directory = DataDirectory::new(data_dir.to_path_buf());
    let source = read_dump_input_source(&directory, "JILL1.VCL")?;
    let DumpInputSource {
        bytes,
        source_size,
        source_sha256,
    } = source;
    let vcl = parse_vcl_source(bytes)?;

    let entries = vcl
        .text_entries()
        .iter()
        .map(|entry| {
            serde_json::json!({
                "index": entry.index(),
                "source_offset": entry.offset(),
                "declared_length": entry.declared_length(),
                "text": entry.text(),
            })
        })
        .collect::<Vec<_>>();

    let json = serde_json::json!({
        "source_file": "JILL1.VCL",
        "source_size": source_size,
        "source_sha256": source_sha256,
        "sound_entries_supported": false,
        "text_entry_count": vcl.text_entry_count(),
        "entries": entries,
    });
    let mut rendered = serde_json::to_string_pretty(&json)
        .map_err(|error| anyhow::anyhow!("failed to serialize dump metadata: {error}"))?;
    rendered.push('\n');
    Ok(rendered)
}

/// Builds metadata JSON for `dump jn`.
fn jn_dump_json(data_dir: &Path) -> Result<String> {
    let directory = DataDirectory::new(data_dir.to_path_buf());
    let required_files = DumpFrameworkKind::Jn.fixed_source_files();
    let mut requested_files = required_files
        .iter()
        .map(|file| (*file).to_string())
        .collect::<Vec<_>>();
    requested_files.extend(
        discover_episode_one_jn1_files(data_dir)?
            .into_iter()
            .filter(|file| {
                !required_files
                    .iter()
                    .any(|required| file.eq_ignore_ascii_case(required))
            }),
    );

    let files = requested_files
        .iter()
        .map(|requested_file| jn_dump_file_json(&directory, requested_file))
        .collect::<Result<Vec<_>>>()?;

    let json = serde_json::json!({
        "data_dir": data_dir.display().to_string(),
        "files": files,
    });
    let mut rendered = serde_json::to_string_pretty(&json)
        .map_err(|error| anyhow::anyhow!("failed to serialize dump metadata: {error}"))?;
    rendered.push('\n');
    Ok(rendered)
}

/// Builds metadata JSON for one `*.JN1` source file.
fn jn_dump_file_json(directory: &DataDirectory, requested_file: &str) -> Result<serde_json::Value> {
    let resolved_path = directory
        .resolve_path_case_insensitive(requested_file)
        .map_err(|error| anyhow::anyhow!("missing input file {requested_file}: {error}"))?;
    let bytes = fs::read(&resolved_path)
        .map_err(|error| anyhow::anyhow!("failed to read input file {requested_file}: {error}"))?;
    let source_size = bytes.len();
    let source_sha256 = sha256_lower_hex(&bytes);
    let mut reader = ByteReader::from_bytes(bytes);
    let jn = JnFile::parse(&mut reader)
        .map_err(|error| anyhow::anyhow!("failed to parse input file {requested_file}: {error}"))?;

    let map_codes = jn.background().map_codes();
    let map_code_min = map_codes.iter().min().copied().unwrap_or(0);
    let map_code_max = map_codes.iter().max().copied().unwrap_or(0);
    let nonzero_cell_count = map_codes.iter().filter(|&&map_code| map_code != 0).count();

    let object_entries = jn
        .objects()
        .iter()
        .map(|entry| {
            serde_json::json!({
                "index": entry.index(),
                "source_offset": entry.offset(),
                "object_type": entry.object_type(),
                "x": entry.x(),
                "y": entry.y(),
                "x_speed": entry.x_speed(),
                "y_speed": entry.y_speed(),
                "width": entry.width(),
                "height": entry.height(),
                "state": entry.state(),
                "sub_state": entry.sub_state(),
                "state_count": entry.state_count(),
                "counter": entry.counter(),
                "flags": entry.flags(),
                "pointer": entry.pointer(),
                "info1": entry.info1(),
                "zap_hold": entry.zap_hold(),
                "string_index": entry.string_index(),
            })
        })
        .collect::<Vec<_>>();

    let string_entries = jn
        .strings()
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            serde_json::json!({
                "index": index,
                "source_offset": entry.offset(),
                "size_in_file": entry.size_in_file(),
                "terminator": entry.terminator(),
                "text": entry.value(),
            })
        })
        .collect::<Vec<_>>();
    let total_bytes_in_file = jn
        .strings()
        .iter()
        .map(openjill_data::jn::JnString::size_in_file)
        .sum::<usize>();

    Ok(serde_json::json!({
        "requested_file": requested_file,
        "resolved_file": path_relative_to_data_dir(directory.as_path(), &resolved_path).display().to_string(),
        "source_size": source_size,
        "source_sha256": source_sha256,
        "background": {
            "width": jn.background().width(),
            "height": jn.background().height(),
            "cell_count": map_codes.len(),
            "map_code_min": map_code_min,
            "map_code_max": map_code_max,
            "nonzero_cell_count": nonzero_cell_count,
        },
        "objects": {
            "count": jn.objects().len(),
            "entries": object_entries,
        },
        "save_data": {
            "source_offset": jn.save_data().offset(),
            "level": jn.save_data().level(),
            "health": jn.save_data().health(),
            "score": jn.save_data().score(),
            "inventory_word_count": jn.save_data().inventory().len(),
        },
        "strings": {
            "count": jn.strings().len(),
            "total_bytes_in_file": total_bytes_in_file,
            "entries": string_entries,
        },
    }))
}

/// Complete deterministic output artifacts generated by one `dump sha` request,
/// including JSON metadata and the indexed atlas PNG payload.
struct ShaDumpOutput {
    /// Pretty-printed metadata JSON serialized from parsed SHA contents.
    metadata_json: String,
    /// Encoded grayscale PNG preserving indexed atlas pixel values.
    atlas_indexed_png: Vec<u8>,
}

/// Intermediate packed SHA atlas containing row-major indexed pixels plus per
/// tile placements, used to bridge packing logic and PNG encoding.
struct ShaAtlasImage {
    /// Atlas width in pixels.
    width: usize,
    /// Atlas height in pixels.
    height: usize,
    /// Row-major indexed atlas pixels.
    pixels: Vec<u8>,
    /// Packed tile placement metadata in parser order.
    tiles: Vec<ShaAtlasTilePlacement>,
}

/// Atlas placement metadata for one tile in parser order.
#[derive(Clone, Copy)]
struct ShaAtlasTilePlacement {
    /// Position of the parent tileset inside `ShaFile::tilesets()`.
    tileset_position: usize,
    /// Position of the tile inside its parent tileset.
    tile_position: usize,
    /// Header entry index for the parent tileset.
    entry_index: usize,
    /// Tile image index inside the parent tileset.
    image_index: usize,
    /// X coordinate of the tile in the atlas.
    x: usize,
    /// Y coordinate of the tile in the atlas.
    y: usize,
    /// Tile width in atlas pixels.
    width: usize,
    /// Tile height in atlas pixels.
    height: usize,
}

/// Builds deterministic SHA metadata and indexed atlas payloads.
fn sha_dump_output(data_dir: &Path) -> Result<ShaDumpOutput> {
    let directory = DataDirectory::new(data_dir.to_path_buf());
    let source = read_dump_input_source(&directory, "JILL1.SHA")?;
    let DumpInputSource {
        bytes,
        source_size,
        source_sha256,
    } = source;
    let sha = parse_sha_source(bytes)?;
    let atlas = pack_sha_tiles_row_major(&sha);
    let atlas_indexed_png = encode_grayscale_png(&atlas)?;

    let mut placement_index = 0usize;
    let tilesets = sha
        .tilesets()
        .iter()
        .enumerate()
        .map(|(tileset_position, tileset)| {
            let color_map = tileset.color_map().map(|entries| {
                entries
                    .iter()
                    .map(|entry| {
                        serde_json::json!({
                            "cga": entry.cga(),
                            "ega": entry.ega(),
                            "vga": entry.vga(),
                            "reserved": entry.reserved(),
                        })
                    })
                    .collect::<Vec<_>>()
            });
            let tile_count = tileset.tiles().len();
            let tiles = tileset
                .tiles()
                .iter()
                .enumerate()
                .map(|(tile_position, tile)| {
                    let placement = atlas
                        .tiles
                        .get(placement_index)
                        .expect("each parsed tile should have an atlas placement");
                    placement_index += 1;
                    debug_assert_eq!(placement.tileset_position, tileset_position);
                    debug_assert_eq!(placement.tile_position, tile_position);
                    debug_assert_eq!(placement.entry_index, tileset.entry_index());
                    debug_assert_eq!(placement.image_index, tile.image_index());
                    serde_json::json!({
                        "image_index": tile.image_index(),
                        "source_offset": tile.offset(),
                        "width": usize::from(tile.width()),
                        "height": usize::from(tile.height()),
                        "data_format": tile.data_format(),
                        "indexed_byte_count": tile.indexed_pixels().len(),
                        "atlas_x": placement.x,
                        "atlas_y": placement.y,
                        "atlas_width": placement.width,
                        "atlas_height": placement.height,
                    })
                })
                .collect::<Vec<_>>();

            serde_json::json!({
                "entry_index": tileset.entry_index(),
                "source_offset": tileset.offset(),
                "declared_size": tileset.size(),
                "tile_count": tileset.tile_count(),
                "image_count": tile_count,
                "rotations": tileset.rotations(),
                "cga_size": tileset.cga_size(),
                "ega_size": tileset.ega_size(),
                "vga_size": tileset.vga_size(),
                "bit_depth": tileset.bit_depth(),
                "flags": tileset.flags(),
                "is_font": tileset.is_font(),
                "is_level_tileset": tileset.is_level_tileset(),
                "color_map_entry_count": color_map.as_ref().map_or(0, Vec::len),
                "color_map": color_map,
                "tiles": tiles,
            })
        })
        .collect::<Vec<_>>();

    let json = serde_json::json!({
        "source_file": "JILL1.SHA",
        "source_size": source_size,
        "source_sha256": source_sha256,
        "header_entry_count": sha.header().entries().len(),
        "valid_header_entry_count": sha.header().valid_entry_count(),
        "tileset_count": sha.tilesets().len(),
        "tilesets": tilesets,
        "atlas_files": [{
            "file_name": SHA_ATLAS_INDEXED_FILE,
            "pixel_format": "indexed-luma8",
            "width": atlas.width,
            "height": atlas.height,
            "tile_padding": SHA_ATLAS_TILE_PADDING,
        }],
    });
    let mut metadata_json = serde_json::to_string_pretty(&json)
        .map_err(|error| anyhow::anyhow!("failed to serialize dump metadata: {error}"))?;
    metadata_json.push('\n');

    Ok(ShaDumpOutput {
        metadata_json,
        atlas_indexed_png,
    })
}

/// Packs all parsed SHA tiles into a deterministic row-major indexed atlas.
fn pack_sha_tiles_row_major(sha: &ShaFile) -> ShaAtlasImage {
    let tile_count = sha
        .tilesets()
        .iter()
        .map(|tileset| tileset.tiles().len())
        .sum();
    if tile_count == 0 {
        return ShaAtlasImage {
            width: 1,
            height: 1,
            pixels: vec![0],
            tiles: Vec::new(),
        };
    }

    let columns = grid_columns(tile_count);
    let mut placements = Vec::with_capacity(tile_count);
    let mut x = 0usize;
    let mut y = 0usize;
    let mut row_height = 0usize;
    let mut row_width = 0usize;
    let mut atlas_width = 0usize;
    let mut column_index = 0usize;

    for (tileset_position, tileset) in sha.tilesets().iter().enumerate() {
        for (tile_position, tile) in tileset.tiles().iter().enumerate() {
            if column_index == columns {
                atlas_width = atlas_width.max(row_width);
                y += row_height + SHA_ATLAS_TILE_PADDING;
                x = 0;
                row_height = 0;
                column_index = 0;
            }

            let width = usize::from(tile.width());
            let height = usize::from(tile.height());
            placements.push(ShaAtlasTilePlacement {
                tileset_position,
                tile_position,
                entry_index: tileset.entry_index(),
                image_index: tile.image_index(),
                x,
                y,
                width,
                height,
            });

            row_height = row_height.max(height);
            row_width = x + width;
            x += width + SHA_ATLAS_TILE_PADDING;
            column_index += 1;
        }
    }
    atlas_width = atlas_width.max(row_width);
    let atlas_height = y + row_height;

    let mut pixels = vec![0u8; atlas_width * atlas_height];
    for placement in &placements {
        let tile = sha.tilesets()[placement.tileset_position].tiles()[placement.tile_position]
            .indexed_pixels();
        for row in 0..placement.height {
            let source_start = row * placement.width;
            let source_end = source_start + placement.width;
            let destination_start = (placement.y + row) * atlas_width + placement.x;
            let destination_end = destination_start + placement.width;
            pixels[destination_start..destination_end]
                .copy_from_slice(&tile[source_start..source_end]);
        }
    }

    ShaAtlasImage {
        width: atlas_width,
        height: atlas_height,
        pixels,
        tiles: placements,
    }
}

/// Returns the smallest square-grid column count that can hold `tile_count`
/// tiles in row-major order, i.e. the smallest `n` where `n * n >= tile_count`.
fn grid_columns(tile_count: usize) -> usize {
    let mut columns = 1usize;
    while columns.saturating_mul(columns) < tile_count {
        columns += 1;
    }
    columns
}

/// Encodes row-major indexed atlas pixels into an 8-bit grayscale PNG byte
/// vector after validating that atlas dimensions fit in `u32`.
fn encode_grayscale_png(atlas: &ShaAtlasImage) -> Result<Vec<u8>> {
    let width = u32::try_from(atlas.width)
        .map_err(|error| anyhow::anyhow!("atlas width does not fit u32: {error}"))?;
    let height = u32::try_from(atlas.height)
        .map_err(|error| anyhow::anyhow!("atlas height does not fit u32: {error}"))?;
    let mut output = Vec::new();
    {
        let mut encoder = Encoder::new(Cursor::new(&mut output), width, height);
        encoder.set_color(ColorType::Grayscale);
        encoder.set_depth(BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|error| anyhow::anyhow!("failed to start PNG encoding: {error}"))?;
        writer
            .write_image_data(&atlas.pixels)
            .map_err(|error| anyhow::anyhow!("failed to encode atlas image: {error}"))?;
    }
    Ok(output)
}

/// Formats a `u16` as a lowercase four-digit hexadecimal string with `0x` prefix.
fn format_u16_hex(value: u16) -> String {
    format!("0x{value:04x}")
}

/// Collects and parses the source files required by a dump kind.
fn collect_dump_sources(kind: DumpFrameworkKind, data_dir: &Path) -> Result<Vec<DumpSourceFile>> {
    let directory = DataDirectory::new(data_dir.to_path_buf());
    let mut requested_files = kind
        .fixed_source_files()
        .iter()
        .map(|file| (*file).to_string())
        .collect::<Vec<_>>();

    if kind == DumpFrameworkKind::Jn {
        requested_files.extend(
            discover_episode_one_jn1_files(data_dir)?
                .into_iter()
                .filter(|file| {
                    !file.eq_ignore_ascii_case("INTRO.JN1") && !file.eq_ignore_ascii_case("MAP.JN1")
                }),
        );
    }

    requested_files
        .into_iter()
        .map(|file| collect_dump_source(kind, &directory, &file))
        .collect()
}

/// Collects and parses one source file for dump framework output.
fn collect_dump_source(
    kind: DumpFrameworkKind,
    directory: &DataDirectory,
    requested_file: &str,
) -> Result<DumpSourceFile> {
    let resolved_path = directory
        .resolve_path_case_insensitive(requested_file)
        .map_err(|error| anyhow::anyhow!("missing input file {requested_file}: {error}"))?;
    let bytes = fs::read(&resolved_path)
        .map_err(|error| anyhow::anyhow!("failed to read input file {requested_file}: {error}"))?;

    parse_dump_source(kind, requested_file, &bytes)?;

    Ok(DumpSourceFile {
        requested_file: requested_file.to_string(),
        resolved_file: path_relative_to_data_dir(directory.as_path(), &resolved_path),
        source_size: bytes.len(),
        source_sha256: sha256_lower_hex(&bytes),
    })
}

/// Parses one source file according to the dump kind for early error reporting.
fn parse_dump_source(kind: DumpFrameworkKind, requested_file: &str, bytes: &[u8]) -> Result<()> {
    match kind {
        DumpFrameworkKind::Dma => {
            DmaFile::from_bytes(bytes.to_vec())
                .map(|_| ())
                .map_err(|error| {
                    anyhow::anyhow!("failed to parse input file {requested_file}: {error}")
                })
        }
        DumpFrameworkKind::Vcl => {
            VclFile::from_bytes(bytes.to_vec())
                .map(|_| ())
                .map_err(|error| {
                    anyhow::anyhow!("failed to parse input file {requested_file}: {error}")
                })
        }
        DumpFrameworkKind::Sha => {
            ShaFile::from_bytes(bytes.to_vec())
                .map(|_| ())
                .map_err(|error| {
                    anyhow::anyhow!("failed to parse input file {requested_file}: {error}")
                })
        }
        DumpFrameworkKind::Jn => JnFile::from_bytes(bytes.to_vec())
            .map(|_| ())
            .map_err(|error| {
                anyhow::anyhow!("failed to parse input file {requested_file}: {error}")
            }),
    }
}

/// Resolves and validates the concrete metadata file path for a dump request.
fn resolve_dump_output_file(request: &DumpRequest) -> Result<PathBuf> {
    reject_original_data_output(&request.output)?;
    if request.kind.writes_directory() {
        validate_directory_dump_output(&request.output)?;
        Ok(request.output.join("metadata.json"))
    } else {
        validate_file_dump_output(&request.output)?;
        Ok(request.output.clone())
    }
}

/// Validates a dump output path that should name a directory.
fn validate_directory_dump_output(output: &Path) -> Result<()> {
    if output.is_file() {
        bail!(
            "dump output {} must be a directory for this dump kind",
            output.display()
        );
    }
    if output.extension().is_some() {
        bail!(
            "dump output {} must be a directory path, not a file-like path",
            output.display()
        );
    }
    Ok(())
}

/// Validates a dump output path that should name a file.
fn validate_file_dump_output(output: &Path) -> Result<()> {
    if output.is_dir() {
        bail!(
            "dump output {} must be a file for this dump kind",
            output.display()
        );
    }
    Ok(())
}

/// Rejects output paths that target the original data tree.
fn reject_original_data_output(output: &Path) -> Result<()> {
    let current_dir = std::env::current_dir()?;
    let workspace_root = find_workspace_root().unwrap_or_else(|| current_dir.clone());
    reject_original_data_output_with_roots(output, &current_dir, &workspace_root)
}

/// Rejects output paths under `data/original` using explicit roots.
fn reject_original_data_output_with_roots(
    output: &Path,
    current_dir: &Path,
    workspace_root: &Path,
) -> Result<()> {
    if !output.is_absolute() && output.starts_with(ORIGINAL_DATA_ROOT) {
        bail!(
            "refusing to write dump output under {}",
            PathBuf::from(ORIGINAL_DATA_ROOT).display()
        );
    }

    let output_abs = normalized_absolute_path(current_dir, output);
    let original_abs = normalized_absolute_path(workspace_root, Path::new(ORIGINAL_DATA_ROOT));
    if output_abs.starts_with(&original_abs) {
        bail!(
            "refusing to write dump output under {}",
            PathBuf::from(ORIGINAL_DATA_ROOT).display()
        );
    }
    Ok(())
}

/// Locates the workspace root by searching for a workspace `Cargo.toml`.
fn find_workspace_root() -> Option<PathBuf> {
    let current_dir = std::env::current_dir().ok()?;
    for ancestor in current_dir.ancestors() {
        let manifest = ancestor.join("Cargo.toml");
        if manifest.is_file()
            && fs::read_to_string(&manifest)
                .ok()
                .is_some_and(|contents| contents.contains("[workspace]"))
        {
            return Some(ancestor.to_path_buf());
        }
    }
    None
}

/// Returns a normalized absolute path for safety checks.
fn normalized_absolute_path(current_dir: &Path, path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        current_dir.join(path)
    };
    normalize_path(&absolute)
}

/// Normalizes `.` and `..` components without requiring the path to exist.
fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(segment) => normalized.push(segment),
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

/// Writes JSON through a temporary file and atomically renames it into place.
fn write_json_file(path: &Path, json: &str, force: bool) -> Result<()> {
    ensure_output_may_be_written(path, force)?;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| anyhow::anyhow!("failed to create {}: {error}", parent.display()))?;
    }

    let tmp_path = path.with_extension(format!(
        "{}.tmp",
        path.extension()
            .and_then(OsStr::to_str)
            .unwrap_or("openjill")
    ));
    fs::write(&tmp_path, json)
        .map_err(|error| anyhow::anyhow!("failed to write {}: {error}", tmp_path.display()))?;
    if force && path.exists() {
        fs::remove_file(path)
            .map_err(|error| anyhow::anyhow!("failed to replace {}: {error}", path.display()))?;
    }
    if let Err(error) = fs::rename(&tmp_path, path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(anyhow::anyhow!(
            "failed to finalize {}: {error}",
            path.display()
        ));
    }
    Ok(())
}

/// Writes `bytes` through a temporary sibling file and atomically renames it
/// into place; when `force` is `true`, any existing destination file is
/// replaced.
fn write_binary_file(path: &Path, bytes: &[u8], force: bool) -> Result<()> {
    ensure_output_may_be_written(path, force)?;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| anyhow::anyhow!("failed to create {}: {error}", parent.display()))?;
    }

    let tmp_path = path.with_extension(format!(
        "{}.tmp",
        path.extension()
            .and_then(OsStr::to_str)
            .unwrap_or("openjill")
    ));
    fs::write(&tmp_path, bytes)
        .map_err(|error| anyhow::anyhow!("failed to write {}: {error}", tmp_path.display()))?;
    if force && path.exists() {
        fs::remove_file(path)
            .map_err(|error| anyhow::anyhow!("failed to replace {}: {error}", path.display()))?;
    }
    if let Err(error) = fs::rename(&tmp_path, path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(anyhow::anyhow!(
            "failed to finalize {}: {error}",
            path.display()
        ));
    }
    Ok(())
}

/// Verifies that a dump output path can be written under the selected overwrite
/// policy before any artifact bytes are committed to disk.
fn ensure_output_may_be_written(path: &Path, force: bool) -> Result<()> {
    if path.exists() && !force {
        bail!(
            "refusing to overwrite existing dump output {} without --force",
            path.display()
        );
    }
    Ok(())
}

/// Builds the framework-level JSON metadata for a dump command.
fn dump_framework_json(
    request: &DumpRequest,
    output_file: &Path,
    sources: &[DumpSourceFile],
) -> Result<String> {
    let source_files = sources
        .iter()
        .map(|source| {
            serde_json::json!({
                "requested_file": &source.requested_file,
                "resolved_file": source.resolved_file.display().to_string(),
                "source_size": source.source_size,
                "source_sha256": &source.source_sha256,
            })
        })
        .collect::<Vec<_>>();

    let json = serde_json::json!({
        "kind": request.kind.as_str(),
        "data_dir": request.data_dir.display().to_string(),
        "output_file": output_file.display().to_string(),
        "payload_implemented": false,
        "source_files": source_files,
    });
    let mut rendered = serde_json::to_string_pretty(&json)
        .map_err(|error| anyhow::anyhow!("failed to serialize dump metadata: {error}"))?;
    rendered.push('\n');
    Ok(rendered)
}

/// Verifies required episode-1 files and discovered `*.JN1` files in `data_dir`.
fn verify_data_directory(data_dir: PathBuf) -> Result<VerificationReport> {
    let directory = DataDirectory::new(data_dir.clone());
    let required_files = REQUIRED_FILES
        .iter()
        .map(|(file, parser_domain)| verify_file(&directory, file, *parser_domain))
        .collect::<Vec<_>>();

    let jn_files = discover_episode_one_jn1_files(directory.as_path())?
        .into_iter()
        .filter(|file_name| {
            !file_name.eq_ignore_ascii_case("INTRO.JN1")
                && !file_name.eq_ignore_ascii_case("MAP.JN1")
        })
        .map(|file_name| verify_file(&directory, &file_name, ParserDomain::Jn))
        .collect::<Vec<_>>();

    Ok(VerificationReport {
        data_dir,
        required_files,
        jn_files,
    })
}

/// Verifies one requested file using case-insensitive path resolution and parser checks.
fn verify_file(
    directory: &DataDirectory,
    requested_file: &str,
    parser_domain: ParserDomain,
) -> FileVerification {
    let resolved_path = match directory.resolve_path_case_insensitive(requested_file) {
        Ok(path) => path,
        Err(DataDirectoryError::FileNotFoundCaseInsensitive { .. }) => {
            return FileVerification {
                requested_file: requested_file.to_string(),
                resolved_file: None,
                parser_domain,
                status: VerificationStatus::Missing,
                size_bytes: None,
                checksum_sha256: None,
                parser_error: None,
            };
        }
        Err(error) => {
            return FileVerification {
                requested_file: requested_file.to_string(),
                resolved_file: None,
                parser_domain,
                status: VerificationStatus::Invalid,
                size_bytes: None,
                checksum_sha256: None,
                parser_error: Some(error.to_string()),
            };
        }
    };

    let bytes = match fs::read(&resolved_path) {
        Ok(bytes) => bytes,
        Err(error) => {
            return FileVerification {
                requested_file: requested_file.to_string(),
                resolved_file: Some(path_relative_to_data_dir(
                    directory.as_path(),
                    &resolved_path,
                )),
                parser_domain,
                status: VerificationStatus::Invalid,
                size_bytes: None,
                checksum_sha256: None,
                parser_error: Some(error.to_string()),
            };
        }
    };

    let checksum_sha256 = sha256_lower_hex(&bytes);
    let size_bytes = bytes.len() as u64;
    match parser_domain.parse(&bytes) {
        Ok(()) => FileVerification {
            requested_file: requested_file.to_string(),
            resolved_file: Some(path_relative_to_data_dir(
                directory.as_path(),
                &resolved_path,
            )),
            parser_domain,
            status: VerificationStatus::Present,
            size_bytes: Some(size_bytes),
            checksum_sha256: Some(checksum_sha256),
            parser_error: None,
        },
        Err(parser_error) => FileVerification {
            requested_file: requested_file.to_string(),
            resolved_file: Some(path_relative_to_data_dir(
                directory.as_path(),
                &resolved_path,
            )),
            parser_domain,
            status: VerificationStatus::Invalid,
            size_bytes: Some(size_bytes),
            checksum_sha256: Some(checksum_sha256),
            parser_error: Some(parser_error),
        },
    }
}

/// Discovers top-level `*.JN1` files in deterministic order.
fn discover_episode_one_jn1_files(data_dir: &Path) -> Result<Vec<String>> {
    if !data_dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut file_names = fs::read_dir(data_dir)?
        .filter_map(std::result::Result::ok)
        .filter_map(|entry| {
            let is_file = entry.file_type().ok()?.is_file();
            if !is_file {
                return None;
            }

            let file_name = entry.file_name();
            let extension = Path::new(&file_name).extension().and_then(OsStr::to_str)?;
            if extension.eq_ignore_ascii_case("jn1") {
                Some(file_name.to_string_lossy().into_owned())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    file_names.sort_by(|left, right| {
        left.to_ascii_lowercase()
            .cmp(&right.to_ascii_lowercase())
            .then_with(|| left.cmp(right))
    });
    Ok(file_names)
}

/// Returns a path relative to `data_dir` when possible.
fn path_relative_to_data_dir(data_dir: &Path, path: &Path) -> PathBuf {
    path.strip_prefix(data_dir)
        .map(Path::to_path_buf)
        .unwrap_or_else(|_| path.to_path_buf())
}

/// Computes a lowercase hexadecimal SHA-256 digest for `bytes`.
fn sha256_lower_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Prints a human-readable verification report.
fn print_verification_report(report: &VerificationReport) {
    println!("data directory: {}", report.data_dir.display());
    for file in &report.required_files {
        print_file_report("required", file);
    }

    if report.jn_files.is_empty() {
        println!("jn_files: none discovered beyond INTRO.JN1 and MAP.JN1");
    } else {
        println!("jn_files:");
        for file in &report.jn_files {
            print_file_report("  discovered", file);
        }
    }

    println!("ok: {}", report.ok());
}

/// Prints one file verification line.
fn print_file_report(label: &str, file: &FileVerification) {
    print!(
        "{label} {} [{} {}]",
        file.requested_file,
        file.status.as_str(),
        file.parser_domain.as_str()
    );

    if let Some(path) = &file.resolved_file {
        print!(" path={}", path.display());
    }
    if let Some(size_bytes) = file.size_bytes {
        print!(" size={size_bytes}");
    }
    if let Some(checksum_sha256) = &file.checksum_sha256 {
        print!(" sha256={checksum_sha256}");
    }
    if let Some(parser_error) = &file.parser_error {
        print!(" error={parser_error}");
    }
    println!();
}

#[cfg(test)]
mod tests {
    use super::{
        Cli, Command, DataCommand, FileVerification, SHA_ATLAS_INDEXED_FILE,
        SHA_ATLAS_TILE_PADDING, VerificationStatus, dispatch,
        reject_original_data_output_with_roots, resolve_data_dir_with_env, verify_data_directory,
    };
    use assert2::check;
    use clap::Parser;
    use png::Decoder;
    use serde_json::Value;
    use std::fs;
    use std::io::Cursor;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Expected SHA-256 digest for the bytes returned by [`valid_dma_bytes`].
    const VALID_DMA_BYTES_SHA256: &str =
        "e2f0bb468adca6ddc6b907bdcec47aa839ec900b69d4d1a632c08b6564225662";

    #[test]
    fn accepts_run_command() {
        let cli = Cli::try_parse_from(["openjill-rs", "run"]).expect("run command should parse");
        check!(matches!(cli.command, Command::Run(_)));
    }

    #[test]
    fn accepts_data_verify_command() {
        let cli = Cli::try_parse_from(["openjill-rs", "data", "verify"])
            .expect("data verify command should parse");
        check!(matches!(
            cli.command,
            Command::Data {
                command: DataCommand::Verify(_)
            }
        ));
    }

    #[test]
    fn accepts_dump_command() {
        let cli =
            Cli::try_parse_from(["openjill-rs", "dump", "dma"]).expect("dump command should parse");
        check!(matches!(cli.command, Command::Dump(_)));
    }

    #[test]
    fn accepts_all_dump_kinds() {
        for kind in ["dma", "vcl", "sha", "jn"] {
            let cli =
                Cli::try_parse_from(["openjill-rs", "dump", kind]).expect("dump kind should parse");
            check!(matches!(cli.command, Command::Dump(_)));
        }
    }

    #[test]
    fn accepts_dump_format_flag() {
        let cli = Cli::try_parse_from(["openjill-rs", "dump", "dma", "--format", "json"])
            .expect("dump format flag should parse");
        check!(matches!(cli.command, Command::Dump(_)));
    }

    /// Unit under test: `verify_data_directory` success path and deterministic checksums.
    ///
    /// Preconditions: all required files are present with synthetically valid
    /// parser input, and additional `*.JN1` files are created in mixed case.
    ///
    /// Invariants asserted: required files are reported as present, additional
    /// JN files are discovered separately, and repeated verification yields
    /// stable SHA-256 checksums.
    #[test]
    fn verifies_required_files_and_discovers_additional_jn_files() {
        let temp_dir = TempDirGuard::new("openjill-cli-data-verify-success");
        write_valid_episode_one_fixture(temp_dir.path());
        fs::write(temp_dir.path().join("level2.jN1"), valid_jn_bytes()).expect("write level2.jN1");

        let first = verify_data_directory(temp_dir.path().to_path_buf())
            .expect("verification should succeed");
        let second = verify_data_directory(temp_dir.path().to_path_buf())
            .expect("repeated verification should succeed");

        check!(first.ok());
        check!(first.required_files.len() == 6);
        check!(
            first
                .required_files
                .iter()
                .all(|file| file.status == VerificationStatus::Present)
        );
        check!(first.jn_files.len() == 1);
        check!(
            first.jn_files[0]
                .requested_file
                .eq_ignore_ascii_case("level2.jn1")
        );

        let dma_first = required_file(&first.required_files, "JILL.DMA");
        let dma_second = required_file(&second.required_files, "JILL.DMA");
        check!(dma_first.checksum_sha256 == dma_second.checksum_sha256);
        check!(let Some(checksum) = &dma_first.checksum_sha256 && checksum == VALID_DMA_BYTES_SHA256);
    }

    /// Unit under test: missing required file reporting.
    ///
    /// Preconditions: a synthetic fixture omits one required fixed file.
    ///
    /// Invariants asserted: verification fails and clearly marks the missing
    /// required file.
    #[test]
    fn reports_missing_required_file() {
        let temp_dir = TempDirGuard::new("openjill-cli-data-verify-missing");
        write_valid_episode_one_fixture(temp_dir.path());
        fs::remove_file(temp_dir.path().join("JILL1.SHA")).expect("remove JILL1.SHA");

        let report = verify_data_directory(temp_dir.path().to_path_buf())
            .expect("verification should complete");
        check!(!report.ok());
        check!(matches!(
            required_file(&report.required_files, "JILL1.SHA").status,
            VerificationStatus::Missing
        ));
    }

    /// Unit under test: invalid required file reporting.
    ///
    /// Preconditions: a synthetic fixture contains a truncated required file.
    ///
    /// Invariants asserted: verification fails, marks that file as invalid, and
    /// surfaces parser-error text.
    #[test]
    fn reports_invalid_required_file() {
        let temp_dir = TempDirGuard::new("openjill-cli-data-verify-invalid");
        write_valid_episode_one_fixture(temp_dir.path());
        fs::write(temp_dir.path().join("JILL1.CFG"), [0u8; 4]).expect("truncate JILL1.CFG");

        let report = verify_data_directory(temp_dir.path().to_path_buf())
            .expect("verification should complete");
        check!(!report.ok());
        let cfg_file = required_file(&report.required_files, "JILL1.CFG");
        check!(cfg_file.status == VerificationStatus::Invalid);
        check!(let Some(error) = &cfg_file.parser_error && error.contains("failed to parse CFG"));
    }

    /// Unit under test: environment-variable fallback in verify data-dir resolution.
    ///
    /// Preconditions: no explicit `--data-dir` value is supplied while an
    /// environment override is provided.
    ///
    /// Invariants asserted: the environment value is used as the resolved
    /// verify data directory.
    #[test]
    fn data_verify_uses_environment_fallback_when_flag_is_omitted() {
        let resolved = resolve_data_dir_with_env(None, Some("/tmp/openjill-fixture".into()));
        check!(resolved == PathBuf::from("/tmp/openjill-fixture"));
    }

    /// Unit under test: environment-variable fallback in dump data-dir resolution.
    ///
    /// Preconditions: no explicit `--data-dir` value is supplied while an
    /// environment override is provided.
    ///
    /// Invariants asserted: the environment value is used as the resolved dump
    /// data directory.
    #[test]
    fn dump_uses_environment_fallback_when_flag_is_omitted() {
        let resolved = resolve_data_dir_with_env(None, Some("/tmp/openjill-fixture".into()));
        check!(resolved == PathBuf::from("/tmp/openjill-fixture"));
    }

    /// Unit under test: failing exit status from `data verify`.
    ///
    /// Preconditions: command dispatch runs `data verify` against a fixture with
    /// a missing required file.
    ///
    /// Invariants asserted: command execution returns an error and names the
    /// missing file in the failure summary.
    #[test]
    fn data_verify_returns_error_when_required_files_are_missing() {
        let temp_dir = TempDirGuard::new("openjill-cli-data-verify-exit");
        write_valid_episode_one_fixture(temp_dir.path());
        fs::remove_file(temp_dir.path().join("MAP.JN1")).expect("remove MAP.JN1");

        let cli = Cli::try_parse_from([
            "openjill-rs",
            "data",
            "verify",
            "--data-dir",
            temp_dir.path().to_string_lossy().as_ref(),
        ])
        .expect("data verify command should parse");

        let Err(error) = dispatch(cli.command) else {
            panic!("verification should fail");
        };

        check!(error.to_string().contains("MAP.JN1"));
    }

    /// Unit under test: failing preflight status from `dump`.
    ///
    /// Preconditions: command dispatch runs `dump` against a fixture with a
    /// missing required file.
    ///
    /// Invariants asserted: command execution returns an error and names the
    /// missing file in the failure summary before dump work starts.
    #[test]
    fn dump_returns_error_when_required_files_are_missing() {
        let temp_dir = TempDirGuard::new("openjill-cli-dump-exit");
        write_valid_episode_one_fixture(temp_dir.path());
        fs::remove_file(temp_dir.path().join("MAP.JN1")).expect("remove MAP.JN1");

        let cli = Cli::try_parse_from([
            "openjill-rs",
            "dump",
            "jn",
            "--data-dir",
            temp_dir.path().to_string_lossy().as_ref(),
        ])
        .expect("dump command should parse");

        let Err(error) = dispatch(cli.command) else {
            panic!("dump preflight should fail");
        };

        check!(error.to_string().contains("MAP.JN1"));
    }

    /// Unit under test: `dump dma` payload rendering.
    ///
    /// Preconditions: a complete synthetic fixture exists and the caller
    /// chooses a file output path outside `data/original/`; the DMA input is
    /// replaced with two entries so ordering and helper fields can be checked.
    ///
    /// Invariants asserted: the command succeeds, writes deterministic
    /// entry-ordered metadata, and includes expected helper fields and names.
    #[test]
    fn dump_writes_dma_payload_metadata_to_selected_file() {
        let temp_dir = TempDirGuard::new("openjill-cli-dump-file");
        write_valid_episode_one_fixture(temp_dir.path());
        fs::write(temp_dir.path().join("jill.dma"), valid_dma_dump_bytes())
            .expect("write custom jill.dma");
        let output = temp_dir.path().join("dma.json");

        let cli = Cli::try_parse_from([
            "openjill-rs",
            "dump",
            "dma",
            "--data-dir",
            temp_dir.path().to_string_lossy().as_ref(),
            "--output",
            output.to_string_lossy().as_ref(),
        ])
        .expect("dump command should parse");

        dispatch(cli.command).expect("dump command should succeed");

        let json = read_json_file(&output);
        check!(json["source_file"] == Value::from("JILL.DMA"));
        check!(json["entry_count"] == Value::from(2));
        check!(json["entries"][0]["index"] == Value::from(0));
        check!(json["entries"][0]["source_offset"] == Value::from(0));
        check!(json["entries"][0]["map_code"] == Value::from(0x0102));
        check!(json["entries"][0]["map_code_hex"] == Value::from("0x0102"));
        check!(json["entries"][0]["flags_hex"] == Value::from("0x0039"));
        check!(json["entries"][0]["is_msg_touch"] == Value::from(true));
        check!(json["entries"][0]["is_stair"] == Value::from(true));
        check!(json["entries"][0]["name"] == Value::from("FLOOR"));
        check!(json["entries"][1]["index"] == Value::from(1));
        check!(json["entries"][1]["source_offset"] == Value::from(12));
        check!(json["entries"][1]["map_code"] == Value::from(0x0304));
        check!(json["entries"][1]["flags_hex"] == Value::from("0x0006"));
        check!(json["entries"][1]["is_stair"] == Value::from(false));
        check!(json["entries"][1]["is_vine"] == Value::from(false));
        check!(json["entries"][1]["name"] == Value::from("LAD"));
    }

    /// Unit under test: `dump vcl` payload rendering.
    ///
    /// Preconditions: a complete synthetic fixture exists and the caller
    /// chooses a file output path outside `data/original/`; the VCL input is
    /// replaced with sparse text entries so parser-order output can be checked.
    ///
    /// Invariants asserted: output includes only non-empty entries in table
    /// order and preserves each entry's original index, declared length, and
    /// decoded text.
    #[test]
    fn dump_writes_vcl_payload_metadata_in_table_order() {
        let temp_dir = TempDirGuard::new("openjill-cli-dump-vcl-file");
        write_valid_episode_one_fixture(temp_dir.path());
        fs::write(temp_dir.path().join("JILL1.vcl"), valid_vcl_dump_bytes())
            .expect("write custom JILL1.vcl");
        let output = temp_dir.path().join("vcl-text.json");

        let cli = Cli::try_parse_from([
            "openjill-rs",
            "dump",
            "vcl",
            "--data-dir",
            temp_dir.path().to_string_lossy().as_ref(),
            "--output",
            output.to_string_lossy().as_ref(),
        ])
        .expect("dump command should parse");

        dispatch(cli.command).expect("dump command should succeed");

        let json = read_json_file(&output);
        check!(json["source_file"] == Value::from("JILL1.VCL"));
        check!(json["sound_entries_supported"] == Value::from(false));
        check!(json["text_entry_count"] == Value::from(2));
        check!(json["entries"][0]["index"] == Value::from(3));
        check!(json["entries"][0]["source_offset"] == Value::from(700));
        check!(json["entries"][0]["declared_length"] == Value::from(5));
        check!(json["entries"][0]["text"] == Value::from("HELLO"));
        check!(json["entries"][1]["index"] == Value::from(10));
        check!(json["entries"][1]["source_offset"] == Value::from(705));
        check!(json["entries"][1]["declared_length"] == Value::from(3));
        check!(json["entries"][1]["text"] == Value::from("BYE"));
    }

    /// Unit under test: `dump sha` payload rendering.
    ///
    /// Preconditions: a complete synthetic fixture exists, the SHA input is
    /// replaced with two valid tilesets and three total images, and output
    /// targets a writable directory outside `data/original/`.
    ///
    /// Invariants asserted: output metadata preserves header and tileset fields,
    /// atlas placement metadata is deterministic and row-major with fixed
    /// padding, and the emitted indexed atlas PNG preserves tile pixel values.
    #[test]
    fn dump_writes_sha_payload_metadata_and_indexed_atlas() {
        let temp_dir = TempDirGuard::new("openjill-cli-dump-sha-dir");
        write_valid_episode_one_fixture(temp_dir.path());
        fs::write(temp_dir.path().join("JILL1.SHA"), valid_sha_dump_bytes())
            .expect("write custom JILL1.SHA");
        let output = temp_dir.path().join("sha-dump");

        let cli = Cli::try_parse_from([
            "openjill-rs",
            "dump",
            "sha",
            "--data-dir",
            temp_dir.path().to_string_lossy().as_ref(),
            "--output",
            output.to_string_lossy().as_ref(),
        ])
        .expect("dump command should parse");

        dispatch(cli.command).expect("dump command should succeed");

        let metadata = read_json_file(&output.join("metadata.json"));
        check!(metadata["source_file"] == Value::from("JILL1.SHA"));
        check!(metadata["header_entry_count"] == Value::from(128));
        check!(metadata["valid_header_entry_count"] == Value::from(2));
        check!(metadata["tileset_count"] == Value::from(2));
        check!(metadata["atlas_files"][0]["file_name"] == Value::from(SHA_ATLAS_INDEXED_FILE));
        check!(metadata["atlas_files"][0]["pixel_format"] == Value::from("indexed-luma8"));
        check!(metadata["atlas_files"][0]["tile_padding"] == Value::from(SHA_ATLAS_TILE_PADDING));
        check!(metadata["atlas_files"][0]["width"] == Value::from(5));
        check!(metadata["atlas_files"][0]["height"] == Value::from(6));

        let first_tileset = &metadata["tilesets"][0];
        check!(first_tileset["entry_index"] == Value::from(2));
        check!(first_tileset["tile_count"] == Value::from(2));
        check!(first_tileset["image_count"] == Value::from(2));
        check!(first_tileset["color_map_entry_count"] == Value::from(4));
        check!(first_tileset["tiles"][0]["image_index"] == Value::from(0));
        check!(first_tileset["tiles"][0]["atlas_x"] == Value::from(0));
        check!(first_tileset["tiles"][0]["atlas_y"] == Value::from(0));
        check!(first_tileset["tiles"][1]["image_index"] == Value::from(1));
        check!(first_tileset["tiles"][1]["atlas_x"] == Value::from(3));
        check!(first_tileset["tiles"][1]["atlas_y"] == Value::from(0));

        let second_tileset = &metadata["tilesets"][1];
        check!(second_tileset["entry_index"] == Value::from(7));
        check!(second_tileset["tile_count"] == Value::from(1));
        check!(second_tileset["image_count"] == Value::from(1));
        check!(second_tileset["color_map_entry_count"] == Value::from(0));
        check!(second_tileset["tiles"][0]["image_index"] == Value::from(0));
        check!(second_tileset["tiles"][0]["atlas_x"] == Value::from(0));
        check!(second_tileset["tiles"][0]["atlas_y"] == Value::from(3));
        check!(second_tileset["tiles"][0]["atlas_width"] == Value::from(3));
        check!(second_tileset["tiles"][0]["atlas_height"] == Value::from(3));

        let atlas_path = output.join(SHA_ATLAS_INDEXED_FILE);
        check!(atlas_path.is_file());
        let (atlas_width, atlas_height, atlas_pixels) = read_grayscale_png(&atlas_path);
        check!(atlas_width == 5);
        check!(atlas_height == 6);
        check!(atlas_pixels[0] == 1);
        check!(atlas_pixels[1] == 2);
        check!(atlas_pixels[5] == 3);
        check!(atlas_pixels[6] == 4);
        check!(atlas_pixels[3] == 5);
        check!(atlas_pixels[4] == 6);
        check!(atlas_pixels[15] == 7);
        check!(atlas_pixels[16] == 8);
        check!(atlas_pixels[17] == 9);
    }

    /// Unit under test: `dump jn` payload rendering.
    ///
    /// Preconditions: a complete synthetic fixture exists, required JN files
    /// are replaced with custom metadata-rich payloads, and additional JN files
    /// are created in mixed case.
    ///
    /// Invariants asserted: the dump contains background/object/save/string
    /// summaries, discovered files are ordered deterministically and
    /// case-insensitively, and output stays metadata-only.
    #[test]
    fn dump_writes_jn_payload_metadata_with_deterministic_discovery_order() {
        let temp_dir = TempDirGuard::new("openjill-cli-dump-jn-dir");
        write_valid_episode_one_fixture(temp_dir.path());
        fs::write(temp_dir.path().join("Intro.Jn1"), valid_jn_dump_bytes())
            .expect("write Intro.Jn1");
        fs::write(temp_dir.path().join("MAP.JN1"), valid_jn_dump_bytes()).expect("write MAP.JN1");
        fs::write(temp_dir.path().join("BETA.jn1"), valid_jn_dump_bytes()).expect("write BETA.jn1");
        fs::write(temp_dir.path().join("alpha.JN1"), valid_jn_dump_bytes())
            .expect("write alpha.JN1");
        let output = temp_dir.path().join("jn-dump");

        let cli = Cli::try_parse_from([
            "openjill-rs",
            "dump",
            "jn",
            "--data-dir",
            temp_dir.path().to_string_lossy().as_ref(),
            "--output",
            output.to_string_lossy().as_ref(),
        ])
        .expect("dump command should parse");

        dispatch(cli.command).expect("dump command should succeed");

        let json = read_json_file(&output.join("metadata.json"));
        check!(json["data_dir"] == Value::from(temp_dir.path().display().to_string()));
        check!(json["files"][0]["requested_file"] == Value::from("INTRO.JN1"));
        check!(json["files"][1]["requested_file"] == Value::from("MAP.JN1"));
        check!(json["files"][2]["requested_file"] == Value::from("alpha.JN1"));
        check!(json["files"][3]["requested_file"] == Value::from("BETA.jn1"));
        check!(json["files"][0]["background"]["width"] == Value::from(128));
        check!(json["files"][0]["background"]["height"] == Value::from(64));
        check!(json["files"][0]["background"]["cell_count"] == Value::from(8192));
        check!(json["files"][0]["background"]["map_code_min"] == Value::from(0));
        check!(json["files"][0]["background"]["map_code_max"] == Value::from(0x0abc));
        check!(json["files"][0]["background"]["nonzero_cell_count"] == Value::from(2));
        check!(json["files"][0]["objects"]["count"] == Value::from(2));
        check!(json["files"][0]["objects"]["entries"][0]["index"] == Value::from(0));
        check!(json["files"][0]["objects"]["entries"][0]["source_offset"] == Value::from(16386));
        check!(json["files"][0]["objects"]["entries"][0]["object_type"] == Value::from(7));
        check!(json["files"][0]["objects"]["entries"][0]["x"] == Value::from(10));
        check!(json["files"][0]["objects"]["entries"][0]["y"] == Value::from(20));
        check!(json["files"][0]["objects"]["entries"][0]["x_speed"] == Value::from(-3));
        check!(json["files"][0]["objects"]["entries"][0]["y_speed"] == Value::from(4));
        check!(json["files"][0]["objects"]["entries"][0]["pointer"] == Value::from(0x1111_1111u64));
        check!(json["files"][0]["objects"]["entries"][0]["string_index"] == Value::from(0));
        check!(json["files"][0]["objects"]["entries"][1]["index"] == Value::from(1));
        check!(json["files"][0]["objects"]["entries"][1]["source_offset"] == Value::from(16417));
        check!(json["files"][0]["objects"]["entries"][1]["string_index"] == Value::Null);
        check!(json["files"][0]["save_data"]["source_offset"] == Value::from(16448));
        check!(json["files"][0]["save_data"]["level"] == Value::from(2));
        check!(json["files"][0]["save_data"]["health"] == Value::from(99));
        check!(json["files"][0]["save_data"]["score"] == Value::from(0x0102_0304u64));
        check!(json["files"][0]["save_data"]["inventory_word_count"] == Value::from(3));
        check!(json["files"][0]["strings"]["count"] == Value::from(2));
        check!(json["files"][0]["strings"]["total_bytes_in_file"] == Value::from(14));
        check!(json["files"][0]["strings"]["entries"][0]["index"] == Value::from(0));
        check!(json["files"][0]["strings"]["entries"][0]["source_offset"] == Value::from(16518));
        check!(json["files"][0]["strings"]["entries"][0]["size_in_file"] == Value::from(8));
        check!(json["files"][0]["strings"]["entries"][0]["terminator"] == Value::from(0));
        check!(json["files"][0]["strings"]["entries"][0]["text"] == Value::from("HELLO"));
        check!(json["files"][0]["strings"]["entries"][1]["index"] == Value::from(1));
        check!(json["files"][0]["strings"]["entries"][1]["source_offset"] == Value::from(16526));
        check!(json["files"][0]["strings"]["entries"][1]["size_in_file"] == Value::from(6));
        check!(json["files"][0]["strings"]["entries"][1]["terminator"] == Value::from(255));
        check!(json["files"][0]["strings"]["entries"][1]["text"] == Value::from("BYE"));
        check!(json["files"][0].get("payload_implemented").is_none());
        check!(json["files"][0].get("source_bytes").is_none());
    }

    /// Unit under test: parser-error reporting for `dump jn`.
    ///
    /// Preconditions: a synthetic fixture contains a truncated discovered JN
    /// file in addition to required files.
    ///
    /// Invariants asserted: command execution fails with parse context naming
    /// the malformed discovered source file.
    #[test]
    fn dump_jn_reports_parser_errors_with_file_context() {
        let temp_dir = TempDirGuard::new("openjill-cli-dump-jn-parse-error");
        write_valid_episode_one_fixture(temp_dir.path());
        fs::write(temp_dir.path().join("BADLEVEL.jn1"), [0u8; 4])
            .expect("write malformed BADLEVEL.jn1");

        let cli = Cli::try_parse_from([
            "openjill-rs",
            "dump",
            "jn",
            "--data-dir",
            temp_dir.path().to_string_lossy().as_ref(),
        ])
        .expect("dump command should parse");

        let Err(error) = dispatch(cli.command) else {
            panic!("dump should fail for malformed jn");
        };

        check!(
            error
                .to_string()
                .contains("failed to parse input file BADLEVEL.jn1")
        );
    }

    /// Unit under test: dump output overwrite safeguards.
    ///
    /// Preconditions: the selected output file already exists.
    ///
    /// Invariants asserted: the command refuses to overwrite the file unless
    /// `--force` is supplied.
    #[test]
    fn dump_refuses_to_overwrite_without_force() {
        let temp_dir = TempDirGuard::new("openjill-cli-dump-overwrite");
        write_valid_episode_one_fixture(temp_dir.path());
        let output = temp_dir.path().join("dma.json");
        fs::write(&output, "{}").expect("write existing output");

        let cli = Cli::try_parse_from([
            "openjill-rs",
            "dump",
            "dma",
            "--data-dir",
            temp_dir.path().to_string_lossy().as_ref(),
            "--output",
            output.to_string_lossy().as_ref(),
        ])
        .expect("dump command should parse");

        let Err(error) = dispatch(cli.command) else {
            panic!("dump should refuse to overwrite");
        };

        check!(error.to_string().contains("--force"));
    }

    /// Unit under test: dump output overwrite with `--force`.
    ///
    /// Preconditions: the selected output file already exists and the caller
    /// supplies `--force`.
    ///
    /// Invariants asserted: the command replaces the output and does not leave
    /// the temporary sibling file behind.
    #[test]
    fn dump_force_overwrites_existing_file() {
        let temp_dir = TempDirGuard::new("openjill-cli-dump-force-overwrite");
        write_valid_episode_one_fixture(temp_dir.path());
        let output = temp_dir.path().join("dma.json");
        fs::write(&output, "{}").expect("write existing output");

        let cli = Cli::try_parse_from([
            "openjill-rs",
            "dump",
            "dma",
            "--data-dir",
            temp_dir.path().to_string_lossy().as_ref(),
            "--output",
            output.to_string_lossy().as_ref(),
            "--force",
        ])
        .expect("dump command should parse");

        dispatch(cli.command).expect("dump command should succeed");

        let json = fs::read_to_string(&output).expect("read dump output");
        check!(json.contains("\"source_file\": \"JILL.DMA\""));
        check!(!output.with_extension("json.tmp").exists());
    }

    /// Unit under test: multi-artifact SHA dump overwrite safeguards.
    ///
    /// Preconditions: the selected SHA output directory already contains an
    /// atlas artifact, while metadata does not exist yet.
    ///
    /// Invariants asserted: the command refuses the request before writing new
    /// metadata, so a failed no-force SHA dump cannot leave partial output.
    #[test]
    fn dump_sha_refuses_existing_atlas_before_writing_metadata() {
        let temp_dir = TempDirGuard::new("openjill-cli-dump-sha-partial-overwrite");
        write_valid_episode_one_fixture(temp_dir.path());
        fs::write(temp_dir.path().join("JILL1.SHA"), valid_sha_dump_bytes())
            .expect("write custom JILL1.SHA");
        let output = temp_dir.path().join("sha-dump");
        fs::create_dir_all(&output).expect("create output directory");
        fs::write(output.join(SHA_ATLAS_INDEXED_FILE), b"existing atlas")
            .expect("write existing atlas");

        let cli = Cli::try_parse_from([
            "openjill-rs",
            "dump",
            "sha",
            "--data-dir",
            temp_dir.path().to_string_lossy().as_ref(),
            "--output",
            output.to_string_lossy().as_ref(),
        ])
        .expect("dump command should parse");

        let Err(error) = dispatch(cli.command) else {
            panic!("dump should refuse to overwrite existing atlas");
        };

        check!(error.to_string().contains("--force"));
        check!(!output.join("metadata.json").exists());
    }

    /// Unit under test: parser-error reporting for `dump dma`.
    ///
    /// Preconditions: a synthetic fixture contains a truncated DMA file.
    ///
    /// Invariants asserted: command execution fails with parse context that
    /// names the failing source file.
    #[test]
    fn dump_dma_reports_parser_errors_with_file_context() {
        let temp_dir = TempDirGuard::new("openjill-cli-dump-dma-parse-error");
        write_valid_episode_one_fixture(temp_dir.path());
        fs::write(temp_dir.path().join("jill.dma"), [0x34, 0x12, 0x01, 0x02])
            .expect("write malformed jill.dma");

        let cli = Cli::try_parse_from([
            "openjill-rs",
            "dump",
            "dma",
            "--data-dir",
            temp_dir.path().to_string_lossy().as_ref(),
        ])
        .expect("dump command should parse");

        let Err(error) = dispatch(cli.command) else {
            panic!("dump should fail for malformed dma");
        };

        check!(
            error
                .to_string()
                .contains("failed to parse input file JILL.DMA")
        );
    }

    /// Unit under test: parser-error reporting for `dump vcl`.
    ///
    /// Preconditions: a synthetic fixture contains a truncated VCL file.
    ///
    /// Invariants asserted: command execution fails with parse context that
    /// names the failing source file.
    #[test]
    fn dump_vcl_reports_parser_errors_with_file_context() {
        let temp_dir = TempDirGuard::new("openjill-cli-dump-vcl-parse-error");
        write_valid_episode_one_fixture(temp_dir.path());
        fs::write(temp_dir.path().join("JILL1.vcl"), [0u8; 20]).expect("write malformed JILL1.vcl");

        let cli = Cli::try_parse_from([
            "openjill-rs",
            "dump",
            "vcl",
            "--data-dir",
            temp_dir.path().to_string_lossy().as_ref(),
        ])
        .expect("dump command should parse");

        let Err(error) = dispatch(cli.command) else {
            panic!("dump should fail for malformed vcl");
        };

        check!(
            error
                .to_string()
                .contains("failed to parse input file JILL1.VCL")
        );
    }

    /// Unit under test: parser-error reporting for `dump sha`.
    ///
    /// Preconditions: a synthetic fixture contains a truncated SHA file.
    ///
    /// Invariants asserted: command execution fails with parse context that
    /// names the failing source file.
    #[test]
    fn dump_sha_reports_parser_errors_with_file_context() {
        let temp_dir = TempDirGuard::new("openjill-cli-dump-sha-parse-error");
        write_valid_episode_one_fixture(temp_dir.path());
        fs::write(temp_dir.path().join("JILL1.SHA"), [0u8; 16]).expect("write malformed JILL1.SHA");

        let cli = Cli::try_parse_from([
            "openjill-rs",
            "dump",
            "sha",
            "--data-dir",
            temp_dir.path().to_string_lossy().as_ref(),
        ])
        .expect("dump command should parse");

        let Err(error) = dispatch(cli.command) else {
            panic!("dump should fail for malformed sha");
        };

        check!(
            error
                .to_string()
                .contains("failed to parse input file JILL1.SHA")
        );
    }

    /// Unit under test: directory dump output validation.
    ///
    /// Preconditions: a SHA dump receives an output path with a file-like
    /// extension.
    ///
    /// Invariants asserted: the command rejects the path instead of creating a
    /// directory named like a JSON file.
    #[test]
    fn directory_dump_rejects_file_like_output_path() {
        let temp_dir = TempDirGuard::new("openjill-cli-dump-dir-file-like");
        write_valid_episode_one_fixture(temp_dir.path());
        let output = temp_dir.path().join("sha.json");

        let cli = Cli::try_parse_from([
            "openjill-rs",
            "dump",
            "sha",
            "--data-dir",
            temp_dir.path().to_string_lossy().as_ref(),
            "--output",
            output.to_string_lossy().as_ref(),
        ])
        .expect("dump command should parse");

        let Err(error) = dispatch(cli.command) else {
            panic!("directory dump should reject file-like output");
        };

        check!(error.to_string().contains("directory path"));
    }

    /// Unit under test: directory dump output validation.
    ///
    /// Preconditions: a JN dump receives an output path that already exists as
    /// a file.
    ///
    /// Invariants asserted: the command rejects the file path before writing.
    #[test]
    fn directory_dump_rejects_existing_file_output_path() {
        let temp_dir = TempDirGuard::new("openjill-cli-dump-dir-existing-file");
        write_valid_episode_one_fixture(temp_dir.path());
        let output = temp_dir.path().join("jn-output");
        fs::write(&output, "not a directory").expect("write existing file");

        let cli = Cli::try_parse_from([
            "openjill-rs",
            "dump",
            "jn",
            "--data-dir",
            temp_dir.path().to_string_lossy().as_ref(),
            "--output",
            output.to_string_lossy().as_ref(),
        ])
        .expect("dump command should parse");

        let Err(error) = dispatch(cli.command) else {
            panic!("directory dump should reject existing file output");
        };

        check!(error.to_string().contains("must be a directory"));
    }

    /// Unit under test: dump output destination safety checks.
    ///
    /// Preconditions: path validation receives a repository-relative path under
    /// `data/original/`.
    ///
    /// Invariants asserted: the output path is rejected without attempting a
    /// dump write.
    #[test]
    fn dump_rejects_original_data_output_path() {
        let workspace = std::env::current_dir().expect("read current directory");

        let error = reject_original_data_output_with_roots(
            Path::new("data/original/dma.json"),
            &workspace,
            &workspace,
        )
        .expect_err("original data output path should be rejected");

        check!(error.to_string().contains("data/original"));
    }

    /// Unit under test: dump output destination safety checks.
    ///
    /// Preconditions: output paths target `data/original/` through absolute and
    /// parent-component forms.
    ///
    /// Invariants asserted: normalized path checks reject both paths.
    #[test]
    fn dump_rejects_normalized_original_data_output_paths() {
        let workspace = std::env::current_dir().expect("read current directory");
        let subdir = workspace.join("crates").join("openjill-cli");
        let absolute_original = workspace.join("data").join("original").join("dma.json");
        let parent_component_original = workspace
            .join("data")
            .join("extracted")
            .join("..")
            .join("original")
            .join("dma.json");

        let absolute_error =
            reject_original_data_output_with_roots(&absolute_original, &subdir, &workspace)
                .expect_err("absolute original path should be rejected");
        let parent_error =
            reject_original_data_output_with_roots(&parent_component_original, &subdir, &workspace)
                .expect_err("parent-component original path should be rejected");

        check!(absolute_error.to_string().contains("data/original"));
        check!(parent_error.to_string().contains("data/original"));
    }

    /// Unit under test: dump output destination safety checks from subdirectories.
    ///
    /// Preconditions: the process-equivalent current directory is a workspace
    /// subdirectory and the caller provides a relative path to `data/original/`.
    ///
    /// Invariants asserted: workspace-root based checks reject the path.
    #[test]
    fn dump_rejects_original_data_output_from_subdirectory_context() {
        let workspace = std::env::current_dir().expect("read current directory");
        let subdir = workspace.join("crates").join("openjill-cli");
        let relative_to_subdir = PathBuf::from("..")
            .join("..")
            .join("data")
            .join("original")
            .join("dma.json");

        let error =
            reject_original_data_output_with_roots(&relative_to_subdir, &subdir, &workspace)
                .expect_err("relative original path should be rejected");

        check!(error.to_string().contains("data/original"));
    }

    /// Returns the matching required file report by requested file name.
    fn required_file<'a>(files: &'a [FileVerification], requested: &str) -> &'a FileVerification {
        files
            .iter()
            .find(|file| file.requested_file == requested)
            .expect("required file should exist in report")
    }

    /// Writes a complete synthetic episode-1 fixture covering all required files.
    fn write_valid_episode_one_fixture(path: &std::path::Path) {
        fs::write(path.join("jill.dma"), valid_dma_bytes()).expect("write jill.dma");
        fs::write(path.join("JILL1.vcl"), valid_vcl_bytes()).expect("write JILL1.vcl");
        fs::write(path.join("JILL1.CFG"), valid_cfg_bytes()).expect("write JILL1.CFG");
        fs::write(path.join("JILL1.SHA"), valid_sha_bytes()).expect("write JILL1.SHA");
        fs::write(path.join("Intro.Jn1"), valid_jn_bytes()).expect("write Intro.Jn1");
        fs::write(path.join("MAP.JN1"), valid_jn_bytes()).expect("write MAP.JN1");
    }

    /// Builds one valid synthetic `JILL.DMA` file with a single entry.
    fn valid_dma_bytes() -> Vec<u8> {
        vec![
            0x01, 0x00, // map_code
            0x02, // tile
            0x03, // tileset
            0x00, 0x00, // flags
            0x01, // name_len
            b'A', // name
        ]
    }

    /// Builds one valid synthetic `JILL1.VCL` file with one non-empty text entry.
    fn valid_vcl_bytes() -> Vec<u8> {
        let mut bytes = vec![0u8; 701];
        bytes[400..404].copy_from_slice(&(700u32).to_le_bytes());
        bytes[560..562].copy_from_slice(&(1u16).to_le_bytes());
        bytes[700] = b'A';
        bytes
    }

    /// Builds a valid synthetic `JILL.DMA` file with two entries for dump-order tests.
    fn valid_dma_dump_bytes() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend(0x0102u16.to_le_bytes());
        bytes.push(0x08);
        bytes.push(0x03);
        bytes.extend(0x0039u16.to_le_bytes());
        bytes.push(5);
        bytes.extend(b"FLOOR");

        bytes.extend(0x0304u16.to_le_bytes());
        bytes.push(0x02);
        bytes.push(0x40);
        bytes.extend(0x0006u16.to_le_bytes());
        bytes.push(3);
        bytes.extend(b"LAD");
        bytes
    }

    /// Builds a valid synthetic `JILL1.VCL` file with sparse non-empty text entries.
    fn valid_vcl_dump_bytes() -> Vec<u8> {
        let mut bytes = vec![0u8; 708];
        bytes[412..416].copy_from_slice(&(700u32).to_le_bytes());
        bytes[440..444].copy_from_slice(&(705u32).to_le_bytes());
        bytes[566..568].copy_from_slice(&(5u16).to_le_bytes());
        bytes[580..582].copy_from_slice(&(3u16).to_le_bytes());
        bytes[700..705].copy_from_slice(b"HELLO");
        bytes[705..708].copy_from_slice(b"BYE");
        bytes
    }

    /// Builds a valid synthetic `JILL1.SHA` file with two tilesets and three
    /// total images for SHA dump metadata and atlas tests.
    fn valid_sha_dump_bytes() -> Vec<u8> {
        const HEADER_ENTRY_COUNT: usize = 128;
        const HEADER_LEN: usize = (HEADER_ENTRY_COUNT * 4) + (HEADER_ENTRY_COUNT * 2);

        let first_tileset_offset = HEADER_LEN;
        let first_tileset = {
            let mut bytes = Vec::new();
            bytes.push(2);
            bytes.extend(1u16.to_le_bytes());
            bytes.extend(4u16.to_le_bytes());
            bytes.extend(4u16.to_le_bytes());
            bytes.extend(4u16.to_le_bytes());
            bytes.push(2);
            bytes.extend(0x0004u16.to_le_bytes());
            bytes.extend([0u8, 1u8, 2u8, 3u8]);
            bytes.extend([4u8, 5u8, 6u8, 7u8]);
            bytes.extend([8u8, 9u8, 10u8, 11u8]);
            bytes.extend([12u8, 13u8, 14u8, 15u8]);
            bytes.extend([2u8, 2u8, 0u8, 1u8, 2u8, 3u8, 4u8]);
            bytes.extend([2u8, 1u8, 0u8, 5u8, 6u8]);
            bytes
        };
        let second_tileset_offset = first_tileset_offset + first_tileset.len();
        let second_tileset = {
            let mut bytes = Vec::new();
            bytes.push(1);
            bytes.extend(2u16.to_le_bytes());
            bytes.extend(3u16.to_le_bytes());
            bytes.extend(3u16.to_le_bytes());
            bytes.extend(3u16.to_le_bytes());
            bytes.push(8);
            bytes.extend(0x0001u16.to_le_bytes());
            bytes.extend([
                3u8, 3u8, 1u8, 7u8, 8u8, 9u8, 10u8, 11u8, 12u8, 13u8, 14u8, 15u8,
            ]);
            bytes
        };

        let mut bytes = vec![0u8; HEADER_LEN];
        bytes.extend_from_slice(&first_tileset);
        bytes.extend_from_slice(&second_tileset);

        let first_offset_bytes = u32::try_from(first_tileset_offset)
            .expect("first tileset offset must fit u32")
            .to_le_bytes();
        bytes[(2 * 4)..(2 * 4 + 4)].copy_from_slice(&first_offset_bytes);
        let second_offset_bytes = u32::try_from(second_tileset_offset)
            .expect("second tileset offset must fit u32")
            .to_le_bytes();
        bytes[(7 * 4)..(7 * 4 + 4)].copy_from_slice(&second_offset_bytes);

        let first_size_bytes = u16::try_from(first_tileset.len())
            .expect("first tileset size must fit u16")
            .to_le_bytes();
        let first_size_start = (HEADER_ENTRY_COUNT * 4) + (2 * 2);
        bytes[first_size_start..first_size_start + 2].copy_from_slice(&first_size_bytes);

        let second_size_bytes = u16::try_from(second_tileset.len())
            .expect("second tileset size must fit u16")
            .to_le_bytes();
        let second_size_start = (HEADER_ENTRY_COUNT * 4) + (7 * 2);
        bytes[second_size_start..second_size_start + 2].copy_from_slice(&second_size_bytes);

        bytes
    }

    /// Builds one valid synthetic `JILL1.CFG` file with defaulted values.
    fn valid_cfg_bytes() -> Vec<u8> {
        vec![0u8; 254]
    }

    /// Builds one valid synthetic `JILL1.SHA` file with an empty valid-entry set.
    fn valid_sha_bytes() -> Vec<u8> {
        vec![0u8; 768]
    }

    /// Builds one valid synthetic `*.JN1` file with empty object and string sections.
    fn valid_jn_bytes() -> Vec<u8> {
        vec![0u8; 16_456]
    }

    /// Builds one valid synthetic `*.JN1` file with nontrivial metadata content.
    fn valid_jn_dump_bytes() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend((0xf234u16).to_le_bytes());
        bytes.extend((0u16).to_le_bytes());
        bytes.extend((0x0abcu16).to_le_bytes());
        bytes.resize(16_384, 0);

        bytes.extend((2u16).to_le_bytes());
        write_jn_object(
            &mut bytes,
            JnObjectFixture {
                object_type: 7,
                x: 10,
                y: 20,
                x_speed: -3,
                y_speed: 4,
                width: 5,
                height: 6,
                state: -7,
                sub_state: 8,
                state_count: 9,
                counter: -10,
                flags: 11,
                pointer: 0x1111_1111,
                info1: -12,
                zap_hold: 13,
            },
        );
        write_jn_object(
            &mut bytes,
            JnObjectFixture {
                object_type: 8,
                x: 30,
                y: 40,
                x_speed: -13,
                y_speed: 14,
                width: 15,
                height: 16,
                state: -17,
                sub_state: 18,
                state_count: 19,
                counter: -20,
                flags: 21,
                pointer: 0,
                info1: -22,
                zap_hold: 23,
            },
        );

        bytes.extend((2u16).to_le_bytes());
        bytes.extend((99u16).to_le_bytes());
        bytes.extend((3u16).to_le_bytes());
        bytes.extend((100u16).to_le_bytes());
        bytes.extend((101u16).to_le_bytes());
        bytes.extend((102u16).to_le_bytes());
        for _ in 0..13 {
            bytes.extend((0u16).to_le_bytes());
        }
        bytes.extend((0x0102_0304u32).to_le_bytes());
        bytes.extend(std::iter::repeat_n(
            0xa5u8,
            openjill_data::jn::SAVE_HOLE_LEN,
        ));

        bytes.extend((5u16).to_le_bytes());
        bytes.extend(b"HELLO");
        bytes.push(0);

        bytes.extend((3u16).to_le_bytes());
        bytes.extend(b"BYE");
        bytes.push(255);
        bytes
    }

    /// Writes one synthetic JN object record in parser field order.
    fn write_jn_object(bytes: &mut Vec<u8>, fixture: JnObjectFixture) {
        bytes.push(fixture.object_type);
        bytes.extend(fixture.x.to_le_bytes());
        bytes.extend(fixture.y.to_le_bytes());
        bytes.extend(fixture.x_speed.to_le_bytes());
        bytes.extend(fixture.y_speed.to_le_bytes());
        bytes.extend(fixture.width.to_le_bytes());
        bytes.extend(fixture.height.to_le_bytes());
        bytes.extend(fixture.state.to_le_bytes());
        bytes.extend(fixture.sub_state.to_le_bytes());
        bytes.extend(fixture.state_count.to_le_bytes());
        bytes.extend(fixture.counter.to_le_bytes());
        bytes.extend(fixture.flags.to_le_bytes());
        bytes.extend(fixture.pointer.to_le_bytes());
        bytes.extend(fixture.info1.to_le_bytes());
        bytes.extend(fixture.zap_hold.to_le_bytes());
    }

    /// Synthetic object fixture used to construct deterministic JN object bytes.
    struct JnObjectFixture {
        /// Object type identifier.
        object_type: u8,
        /// Object X coordinate.
        x: u16,
        /// Object Y coordinate.
        y: u16,
        /// Signed horizontal speed.
        x_speed: i16,
        /// Signed vertical speed.
        y_speed: i16,
        /// Object width.
        width: u16,
        /// Object height.
        height: u16,
        /// Signed state value.
        state: i16,
        /// Unsigned sub-state value.
        sub_state: u16,
        /// Unsigned state counter.
        state_count: u16,
        /// Signed counter value.
        counter: i16,
        /// Object flags.
        flags: u16,
        /// Raw string pointer marker.
        pointer: u32,
        /// Signed auxiliary field.
        info1: i16,
        /// Unsigned zap/hold field.
        zap_hold: u16,
    }

    /// Reads and deserializes a JSON file written by dump commands.
    fn read_json_file(path: &Path) -> Value {
        let bytes = fs::read(path).expect("read dump output");
        serde_json::from_slice(&bytes).expect("parse dump output json")
    }

    /// Reads a grayscale PNG file and returns dimensions plus row-major pixels.
    fn read_grayscale_png(path: &Path) -> (usize, usize, Vec<u8>) {
        let png_bytes = fs::read(path).expect("read atlas png");
        let cursor = Cursor::new(png_bytes);
        let decoder = Decoder::new(cursor);
        let mut reader = decoder.read_info().expect("decode atlas header");
        let mut decode_buffer = vec![0; reader.output_buffer_size().expect("output size")];
        let output = reader
            .next_frame(&mut decode_buffer)
            .expect("decode atlas frame");
        check!(output.color_type == png::ColorType::Grayscale);
        check!(output.bit_depth == png::BitDepth::Eight);
        let used = output.buffer_size();
        (
            output.width as usize,
            output.height as usize,
            decode_buffer[..used].to_vec(),
        )
    }

    /// Owned temporary directory guard that removes its path on drop.
    struct TempDirGuard(PathBuf);

    impl TempDirGuard {
        /// Creates a unique temporary directory with the supplied `prefix`.
        fn new(prefix: &str) -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock should be after epoch")
                .as_nanos();
            let path = std::env::temp_dir().join(format!("{prefix}-{nanos}"));
            fs::create_dir_all(&path).expect("create temp directory");
            Self(path)
        }

        /// Returns the temporary directory path.
        fn path(&self) -> &std::path::Path {
            self.0.as_path()
        }
    }

    impl Drop for TempDirGuard {
        /// Removes the temporary directory tree as a best-effort cleanup.
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}
