use std::error::Error as _;
use std::path::Path;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use semver::Version;
use thiserror::Error;
use xtask::check::CheckKind;
use xtask::targets::TargetsArgs;

#[derive(Debug, Parser)]
#[command(about = "Repository automation for rsproxy")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run repository structure and workflow checks.
    Check(CheckArgs),
    /// Synchronize the Cargo and npm release version.
    Release(ReleaseArgs),
    /// Validate coverage, performance, and stability reports.
    Targets(TargetsArgs),
}

#[derive(Debug, Args)]
struct CheckArgs {
    /// Check group to run.
    #[arg(value_enum)]
    kind: CheckKind,
}

#[derive(Debug, Args)]
struct ReleaseArgs {
    /// Semantic version to apply or verify.
    version: Version,

    /// Verify consistency without changing files.
    #[arg(long)]
    check: bool,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("xtask: {error}");
            let mut source = error.source();
            while let Some(cause) = source {
                eprintln!("  caused by: {cause}");
                source = cause.source();
            }
            ExitCode::FAILURE
        }
    }
}

#[derive(Debug, Error)]
enum XtaskError {
    #[error(transparent)]
    Check(#[from] xtask::check::CheckError),
    #[error(transparent)]
    Release(#[from] xtask::release::ReleaseError),
    #[error(transparent)]
    Target(#[from] xtask::targets::TargetError),
}

fn run() -> Result<(), XtaskError> {
    let cli = Cli::parse();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("xtask must live under <workspace>/crates/xtask");

    match cli.command {
        Command::Check(args) => {
            let report = xtask::check::run(root, args.kind)?;
            for check in report.checks {
                println!("{}: {}", check.kind, check.summary);
            }
        }
        Command::Release(args) => {
            let outcome = xtask::release::release(root, &args.version, args.check)?;
            if args.check {
                println!("release {}: manifests are synchronized", args.version);
            } else if outcome.changed_files == 0 {
                println!("release {}: already synchronized", args.version);
            } else {
                println!(
                    "release {}: updated {} file(s)",
                    args.version, outcome.changed_files
                );
            }
        }
        Command::Targets(args) => {
            let outcome = xtask::targets::run(&args)?;
            println!("{outcome}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
