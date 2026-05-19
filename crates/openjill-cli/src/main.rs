#![forbid(unsafe_code)]

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand, ValueEnum};
use openjill_audio::AudioBackend;
use openjill_core::CoreState;
use openjill_data::cfg::CfgFile;
use openjill_data::dma::DmaFile;
use openjill_data::jn::JnFile;
use openjill_data::sha::ShaFile;
use openjill_data::vcl::VclFile;
use openjill_data::{DataDirectory, DataDirectoryError};
use openjill_game::GameApp;
use openjill_render::Renderer;
use sha2::{Digest, Sha256};

/// Environment variable used as a fallback data directory for `data verify` and `dump`.
const OPENJILL_DATA_DIR_ENV: &str = "OPENJILL_DATA_DIR";
/// Workspace-relative fallback path used when no flag or environment override is supplied.
const DEFAULT_DATA_DIR: &str = "data/original/JILL1";
/// Default output path for `dump dma` relative to the current working directory.
const DUMP_DMA_OUTPUT: &str = "data/extracted/JILL1/debug/dma.json";
/// Default output path for `dump vcl` relative to the current working directory.
const DUMP_VCL_OUTPUT: &str = "data/extracted/JILL1/debug/vcl-text.json";
/// Default output directory for `dump sha` relative to the current working directory.
const DUMP_SHA_OUTPUT: &str = "data/extracted/JILL1/debug/sha";
/// Default output directory for `dump jn` relative to the current working directory.
const DUMP_JN_OUTPUT: &str = "data/extracted/JILL1/debug/jn";
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
    /// Produce debug dumps of episode 1 data files.
    Dump {
        #[command(subcommand)]
        command: DumpCommand,
    },
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

/// Subcommands available under the `dump` command group.
#[derive(Debug, Subcommand)]
enum DumpCommand {
    /// Dump `JILL.DMA` map-code table metadata as JSON.
    Dma(DumpKindArgs),
    /// Dump `JILL1.VCL` text-entry metadata as JSON.
    Vcl(DumpKindArgs),
    /// Dump `JILL1.SHA` tileset and atlas metadata as JSON.
    Sha(DumpKindArgs),
    /// Dump `*.JN1` level and save metadata as JSON.
    Jn(DumpKindArgs),
}

/// Output format for `dump` subcommands.
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum DumpFormat {
    /// Structured JSON output.
    Json,
}

/// Shared arguments accepted by every `dump` subcommand.
#[derive(Args, Debug)]
struct DumpKindArgs {
    /// Data directory containing episode 1 game files.
    ///
    /// Resolved in order: this flag, then `OPENJILL_DATA_DIR`, then
    /// `data/original/JILL1`.
    #[arg(long)]
    data_dir: Option<PathBuf>,

    /// Output path or directory for the dump.
    ///
    /// Defaults to a kind-specific path under
    /// `data/extracted/JILL1/debug/`.
    #[arg(long)]
    output: Option<PathBuf>,

    /// Output format; currently only `json` is supported.
    #[arg(long, default_value = "json")]
    format: DumpFormat,

    /// Overwrite an existing output file without prompting.
    #[arg(long)]
    force: bool,
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
        Command::Dump { command } => dispatch_dump(command),
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

/// Dispatches a `dump` subcommand to the appropriate per-kind handler.
fn dispatch_dump(command: DumpCommand) -> Result<()> {
    match command {
        DumpCommand::Dma(args) => dump_dma_command(args),
        DumpCommand::Vcl(args) => dump_vcl_command(args),
        DumpCommand::Sha(args) => dump_sha_command(args),
        DumpCommand::Jn(args) => dump_jn_command(args),
    }
}

/// Runs the `dump dma` command stub, validating data-dir and output paths.
///
/// Opens `JILL.DMA` to surface missing-input errors early. The actual
/// serialization payload is implemented in a later issue.
fn dump_dma_command(args: DumpKindArgs) -> Result<()> {
    let data_dir = resolve_dump_data_dir(args.data_dir);
    let output = args.output.unwrap_or_else(|| PathBuf::from(DUMP_DMA_OUTPUT));
    let workspace_root = find_workspace_root();
    validate_dump_output_path(&output, workspace_root.as_deref())?;

    let directory = DataDirectory::new(data_dir.clone());
    let _reader = directory
        .open_reader("JILL.DMA")
        .with_context(|| format!("failed to open JILL.DMA from {}", data_dir.display()))?;

    println!(
        "'dump dma' payload is not yet implemented (output: {})",
        output.display()
    );
    Ok(())
}

/// Runs the `dump vcl` command stub, validating data-dir and output paths.
///
/// Opens `JILL1.VCL` to surface missing-input errors early. The actual
/// serialization payload is implemented in a later issue.
fn dump_vcl_command(args: DumpKindArgs) -> Result<()> {
    let data_dir = resolve_dump_data_dir(args.data_dir);
    let output = args.output.unwrap_or_else(|| PathBuf::from(DUMP_VCL_OUTPUT));
    let workspace_root = find_workspace_root();
    validate_dump_output_path(&output, workspace_root.as_deref())?;

    let directory = DataDirectory::new(data_dir.clone());
    let _reader = directory
        .open_reader("JILL1.VCL")
        .with_context(|| format!("failed to open JILL1.VCL from {}", data_dir.display()))?;

    println!(
        "'dump vcl' payload is not yet implemented (output: {})",
        output.display()
    );
    Ok(())
}

/// Runs the `dump sha` command stub, validating data-dir and output paths.
///
/// Opens `JILL1.SHA` to surface missing-input errors early. The actual
/// serialization payload is implemented in a later issue.
fn dump_sha_command(args: DumpKindArgs) -> Result<()> {
    let data_dir = resolve_dump_data_dir(args.data_dir);
    let output = args.output.unwrap_or_else(|| PathBuf::from(DUMP_SHA_OUTPUT));
    let workspace_root = find_workspace_root();
    validate_dump_output_path(&output, workspace_root.as_deref())?;

    let directory = DataDirectory::new(data_dir.clone());
    let _reader = directory
        .open_reader("JILL1.SHA")
        .with_context(|| format!("failed to open JILL1.SHA from {}", data_dir.display()))?;

    println!(
        "'dump sha' payload is not yet implemented (output: {})",
        output.display()
    );
    Ok(())
}

/// Runs the `dump jn` command stub, validating data-dir and output paths.
///
/// Opens `INTRO.JN1` to surface missing-input errors early. The actual
/// serialization payload is implemented in a later issue.
fn dump_jn_command(args: DumpKindArgs) -> Result<()> {
    let data_dir = resolve_dump_data_dir(args.data_dir);
    let output = args.output.unwrap_or_else(|| PathBuf::from(DUMP_JN_OUTPUT));
    let workspace_root = find_workspace_root();
    validate_dump_output_path(&output, workspace_root.as_deref())?;

    let directory = DataDirectory::new(data_dir.clone());
    let _reader = directory
        .open_reader("INTRO.JN1")
        .with_context(|| format!("failed to open INTRO.JN1 from {}", data_dir.display()))?;

    println!(
        "'dump jn' payload is not yet implemented (output: {})",
        output.display()
    );
    Ok(())
}

/// Resolves the dump data directory using the CLI flag, env variable, and default fallback.
fn resolve_dump_data_dir(explicit: Option<PathBuf>) -> PathBuf {
    resolve_dump_data_dir_with_env(explicit, std::env::var_os(OPENJILL_DATA_DIR_ENV))
}

/// Inner helper that resolves the dump data directory with an explicit env override for testability.
fn resolve_dump_data_dir_with_env(
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

/// Locates the workspace root by walking parent directories looking for a
/// `Cargo.toml` file that contains a `[workspace]` section.
///
/// Returns `None` when the workspace root cannot be found from the current
/// working directory.
fn find_workspace_root() -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    let mut current = cwd.as_path();
    loop {
        let cargo_toml = current.join("Cargo.toml");
        if cargo_toml.is_file()
            && fs::read_to_string(&cargo_toml)
                .ok()
                .is_some_and(|c| c.contains("[workspace]"))
        {
            return Some(current.to_path_buf());
        }
        current = current.parent()?;
    }
}

/// Canonicalizes `path`, falling back to the nearest existing ancestor when the
/// path itself does not yet exist on disk.
///
/// This makes it safe to compare paths that refer to future output files in
/// directories that have not been created yet.
fn canonicalize_or_ancestor(path: &Path) -> PathBuf {
    if let Ok(p) = path.canonicalize() {
        return p;
    }
    let mut remaining: Vec<std::ffi::OsString> = Vec::new();
    let mut current = path;
    while let Some(parent) = current.parent() {
        // Always accumulate the current component name before checking whether
        // the parent is canonicalizable, so it is included in the result.
        if let Some(name) = current.file_name() {
            remaining.push(name.to_owned());
        }
        if let Ok(canonical_parent) = parent.canonicalize() {
            let mut result = canonical_parent;
            for component in remaining.into_iter().rev() {
                result = result.join(component);
            }
            return result;
        }
        current = parent;
    }
    path.to_path_buf()
}

/// Validates that `output` is a safe destination for a dump file.
///
/// Rejects paths inside `data/original/` (proprietary game data) and paths
/// inside the workspace that are outside `data/extracted/` (tracked sources).
/// Passes paths that are either outside the workspace entirely or under
/// `data/extracted/`.
///
/// `workspace_root` is `None` when no workspace can be detected, in which case
/// no workspace-relative checks are performed.
fn validate_dump_output_path(output: &Path, workspace_root: Option<&Path>) -> Result<()> {
    let canonical_output = canonicalize_or_ancestor(output);

    if let Some(workspace) = workspace_root {
        let canonical_workspace = canonicalize_or_ancestor(workspace);
        let original_data = canonical_workspace.join("data").join("original");
        let extracted_data = canonical_workspace.join("data").join("extracted");

        if canonical_output.starts_with(&original_data) {
            bail!(
                "output path {} is inside data/original/ which contains proprietary game data; \
                 choose a path under data/extracted/ or outside the workspace",
                output.display()
            );
        }

        if canonical_output.starts_with(&canonical_workspace)
            && !canonical_output.starts_with(&extracted_data)
        {
            bail!(
                "output path {} is inside the workspace but outside data/extracted/; \
                 choose a path under data/extracted/ or outside the workspace",
                output.display()
            );
        }
    }

    Ok(())
}

/// Writes `content` to `output` atomically using a temporary sibling file and rename.
///
/// Creates missing parent directories after path checks have passed.  Rejects
/// writing when `output` already exists and `force` is `false`.  The temporary
/// file is removed on rename failure to avoid leaving partial output on disk.
///
/// This function is part of the dump framework and will be called by individual
/// dump-kind implementations introduced in later issues.
#[allow(dead_code)]
fn write_dump_atomic(output: &Path, content: &[u8], force: bool) -> Result<()> {
    if output.exists() && !force {
        bail!(
            "output file {} already exists; use --force to overwrite",
            output.display()
        );
    }

    let parent = output.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create output directory {}", parent.display()))?;

    let file_name = output
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("dump");
    let tmp_path = parent.join(format!(".{file_name}.tmp"));

    fs::write(&tmp_path, content)
        .with_context(|| format!("failed to write temporary file {}", tmp_path.display()))?;

    fs::rename(&tmp_path, output).map_err(|error| {
        let _ = fs::remove_file(&tmp_path);
        anyhow::anyhow!(
            "failed to rename {} to {}: {error}",
            tmp_path.display(),
            output.display()
        )
    })
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
    resolve_verify_data_dir_with_env(
        args.data_dir.clone(),
        std::env::var_os(OPENJILL_DATA_DIR_ENV),
    )
}

/// Resolves data-directory overrides for verification.
fn resolve_verify_data_dir_with_env(
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
        Cli, Command, DataCommand, DumpCommand, FileVerification, VerificationStatus,
        DUMP_DMA_OUTPUT, DUMP_JN_OUTPUT, DUMP_SHA_OUTPUT, DUMP_VCL_OUTPUT, dispatch,
        resolve_dump_data_dir_with_env, resolve_verify_data_dir_with_env,
        validate_dump_output_path, verify_data_directory, write_dump_atomic,
    };
    use assert2::check;
    use clap::Parser;
    use std::fs;
    use std::path::PathBuf;
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
    fn accepts_dump_dma_command() {
        let cli = Cli::try_parse_from(["openjill-rs", "dump", "dma"])
            .expect("dump dma command should parse");
        check!(matches!(
            cli.command,
            Command::Dump {
                command: DumpCommand::Dma(_)
            }
        ));
    }

    #[test]
    fn accepts_dump_vcl_command() {
        let cli = Cli::try_parse_from(["openjill-rs", "dump", "vcl"])
            .expect("dump vcl command should parse");
        check!(matches!(
            cli.command,
            Command::Dump {
                command: DumpCommand::Vcl(_)
            }
        ));
    }

    #[test]
    fn accepts_dump_sha_command() {
        let cli = Cli::try_parse_from(["openjill-rs", "dump", "sha"])
            .expect("dump sha command should parse");
        check!(matches!(
            cli.command,
            Command::Dump {
                command: DumpCommand::Sha(_)
            }
        ));
    }

    #[test]
    fn accepts_dump_jn_command() {
        let cli = Cli::try_parse_from(["openjill-rs", "dump", "jn"])
            .expect("dump jn command should parse");
        check!(matches!(
            cli.command,
            Command::Dump {
                command: DumpCommand::Jn(_)
            }
        ));
    }

    /// Unit under test: default output path constants for every dump kind.
    ///
    /// Preconditions: no `--output` flag is supplied.
    ///
    /// Invariants asserted: every default output path is rooted under the
    /// approved `data/extracted/` directory.
    #[test]
    fn dump_default_output_paths_are_under_data_extracted() {
        check!(DUMP_DMA_OUTPUT.starts_with("data/extracted/"));
        check!(DUMP_VCL_OUTPUT.starts_with("data/extracted/"));
        check!(DUMP_SHA_OUTPUT.starts_with("data/extracted/"));
        check!(DUMP_JN_OUTPUT.starts_with("data/extracted/"));
    }

    /// Unit under test: dump data-dir environment fallback.
    ///
    /// Preconditions: no `--data-dir` flag is supplied; an env override is present.
    ///
    /// Invariants asserted: the resolved data directory equals the env value.
    #[test]
    fn dump_uses_environment_fallback_when_flag_is_omitted() {
        let resolved =
            resolve_dump_data_dir_with_env(None, Some("/tmp/openjill-data".into()));
        check!(resolved == PathBuf::from("/tmp/openjill-data"));
    }

    /// Unit under test: dump data-dir explicit flag precedence over env.
    ///
    /// Preconditions: both `--data-dir` and an env override are present.
    ///
    /// Invariants asserted: the explicit flag value wins.
    #[test]
    fn dump_uses_explicit_flag_over_environment() {
        let resolved = resolve_dump_data_dir_with_env(
            Some(PathBuf::from("/explicit/data")),
            Some("/env/data".into()),
        );
        check!(resolved == PathBuf::from("/explicit/data"));
    }

    /// Unit under test: `validate_dump_output_path` rejection of `data/original/`.
    ///
    /// Preconditions: the output path is inside the workspace `data/original/`
    /// directory.
    ///
    /// Invariants asserted: validation returns an error whose message mentions
    /// `data/original/`.
    #[test]
    fn dump_rejects_output_inside_data_original() {
        let workspace = PathBuf::from(env!("CARGO_WORKSPACE_DIR"));
        let bad_path = workspace.join("data/original/out.json");
        let result = validate_dump_output_path(&bad_path, Some(&workspace));
        check!(result.is_err());
        check!(result.unwrap_err().to_string().contains("data/original/"));
    }

    /// Unit under test: `validate_dump_output_path` rejection of tracked workspace paths.
    ///
    /// Preconditions: the output path is inside the workspace `crates/` directory
    /// which is tracked source but outside `data/extracted/`.
    ///
    /// Invariants asserted: validation returns an error.
    #[test]
    fn dump_rejects_workspace_path_outside_data_extracted() {
        let workspace = PathBuf::from(env!("CARGO_WORKSPACE_DIR"));
        let bad_path = workspace.join("crates/out.json");
        let result = validate_dump_output_path(&bad_path, Some(&workspace));
        check!(result.is_err());
    }

    /// Unit under test: `validate_dump_output_path` acceptance of `data/extracted/` paths.
    ///
    /// Preconditions: the output path is inside the workspace `data/extracted/`
    /// directory (the approved in-repo dump location).
    ///
    /// Invariants asserted: validation succeeds.
    #[test]
    fn dump_accepts_output_inside_data_extracted() {
        let workspace = PathBuf::from(env!("CARGO_WORKSPACE_DIR"));
        let ok_path = workspace.join("data/extracted/JILL1/debug/dma.json");
        let result = validate_dump_output_path(&ok_path, Some(&workspace));
        check!(result.is_ok());
    }

    /// Unit under test: `validate_dump_output_path` acceptance of outside-workspace paths.
    ///
    /// Preconditions: the output path is under the system temporary directory,
    /// which is outside any workspace.
    ///
    /// Invariants asserted: validation succeeds for paths entirely outside the
    /// workspace.
    #[test]
    fn dump_accepts_output_outside_workspace() {
        let workspace = PathBuf::from(env!("CARGO_WORKSPACE_DIR"));
        let ok_path = std::env::temp_dir().join("openjill-dump-test-output.json");
        let result = validate_dump_output_path(&ok_path, Some(&workspace));
        check!(result.is_ok());
    }

    /// Unit under test: `write_dump_atomic` overwrite rejection without `--force`.
    ///
    /// Preconditions: the output file already exists on disk.
    ///
    /// Invariants asserted: the write fails with an error mentioning `--force`.
    #[test]
    fn dump_rejects_overwrite_without_force() {
        let temp_dir = TempDirGuard::new("openjill-cli-dump-overwrite-reject");
        let path = temp_dir.path().join("existing.json");
        fs::write(&path, b"existing content").expect("write existing file");

        let result = write_dump_atomic(&path, b"new content", false);
        check!(result.is_err());
        check!(result.unwrap_err().to_string().contains("--force"));
    }

    /// Unit under test: `write_dump_atomic` overwrite acceptance with `--force`.
    ///
    /// Preconditions: the output file already exists on disk; `force` is `true`.
    ///
    /// Invariants asserted: the write succeeds and the file contains the new
    /// content.
    #[test]
    fn dump_allows_overwrite_with_force() {
        let temp_dir = TempDirGuard::new("openjill-cli-dump-overwrite-allow");
        let path = temp_dir.path().join("existing.json");
        fs::write(&path, b"existing content").expect("write existing file");

        let result = write_dump_atomic(&path, b"new content", true);
        check!(result.is_ok());
        check!(fs::read(&path).expect("read output") == b"new content");
    }

    /// Unit under test: `dump dma` missing input file error.
    ///
    /// Preconditions: a data directory is supplied that does not contain `JILL.DMA`.
    ///
    /// Invariants asserted: the command returns an error whose message names
    /// `JILL.DMA`.
    #[test]
    fn dump_dma_reports_error_for_missing_input_file() {
        let temp_dir = TempDirGuard::new("openjill-cli-dump-dma-missing");
        let out_path = temp_dir.path().join("out.json");
        let cli = Cli::try_parse_from([
            "openjill-rs",
            "dump",
            "dma",
            "--data-dir",
            temp_dir.path().to_string_lossy().as_ref(),
            "--output",
            out_path.to_string_lossy().as_ref(),
        ])
        .expect("dump dma should parse");

        let Err(error) = dispatch(cli.command) else {
            panic!("should fail when JILL.DMA is missing");
        };
        check!(error.to_string().contains("JILL.DMA"));
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
        let resolved = resolve_verify_data_dir_with_env(None, Some("/tmp/openjill-fixture".into()));
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
