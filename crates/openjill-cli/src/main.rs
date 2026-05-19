#![forbid(unsafe_code)]

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use clap::{Args, Parser, Subcommand};
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

/// Environment variable used as a fallback data directory for `data verify`.
const OPENJILL_DATA_DIR_ENV: &str = "OPENJILL_DATA_DIR";
/// Workspace-relative fallback path used when no flag or environment override is supplied.
const DEFAULT_DATA_DIR: &str = "data/original/JILL1";
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
    #[command(flatten)]
    common: DataDirArgs,
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
    println!(
        "'dump' command is currently a workspace-foundation stub for {}.",
        args.common.data_dir.display()
    );
    Ok(())
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
        Cli, Command, DataCommand, FileVerification, VerificationStatus, dispatch,
        resolve_verify_data_dir_with_env, verify_data_directory,
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
    fn accepts_dump_command() {
        let cli = Cli::try_parse_from(["openjill-rs", "dump"]).expect("dump command should parse");
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
