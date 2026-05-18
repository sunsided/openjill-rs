#![forbid(unsafe_code)]

use std::path::PathBuf;

use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use openjill_audio::AudioBackend;
use openjill_core::CoreState;
use openjill_data::DataDirectory;
use openjill_game::GameApp;
use openjill_render::Renderer;

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
    #[command(flatten)]
    common: DataDirArgs,
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
    println!(
        "'data verify' command is currently a workspace-foundation stub for {}.",
        args.common.data_dir.display()
    );
    Ok(())
}

fn dump_command(args: DumpArgs) -> Result<()> {
    println!(
        "'dump' command is currently a workspace-foundation stub for {}.",
        args.common.data_dir.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Cli, Command, DataCommand};
    use clap::Parser;

    #[test]
    fn accepts_run_command() {
        let cli = Cli::try_parse_from(["openjill-rs", "run"]).expect("run command should parse");
        assert!(matches!(cli.command, Command::Run(_)));
    }

    #[test]
    fn accepts_data_verify_command() {
        let cli = Cli::try_parse_from(["openjill-rs", "data", "verify"])
            .expect("data verify command should parse");
        assert!(matches!(
            cli.command,
            Command::Data {
                command: DataCommand::Verify(_)
            }
        ));
    }

    #[test]
    fn accepts_dump_command() {
        let cli = Cli::try_parse_from(["openjill-rs", "dump"]).expect("dump command should parse");
        assert!(matches!(cli.command, Command::Dump(_)));
    }
}
