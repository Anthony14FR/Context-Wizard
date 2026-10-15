//! The `wizard` binary: hook commands and the MCP server of Context Wizard.

mod decide;
mod event;
mod hook;
mod index;
mod install;
mod opt_in;
mod status;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};

/// Command-line interface of the `wizard` binary.
#[derive(Debug, Parser)]
#[command(name = "wizard", version = wizard_core::VERSION, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

/// Subcommands of the `wizard` binary.
#[derive(Debug, Subcommand)]
enum Command {
    /// Handles one Claude Code hook event read from standard input.
    Hook,
    /// Enables or disables Context Wizard for a project.
    Decide {
        /// The decision to record.
        answer: Answer,
        /// Project root. Defaults to the current directory.
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
    /// Builds or updates the index of an enabled project.
    Index {
        /// Project root. Defaults to the current directory.
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
    /// Shows whether Context Wizard is enabled for a project and whether its binary is healthy.
    Status {
        /// Project root. Defaults to the current directory.
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
}

/// Answer to the opt-in question.
#[derive(Debug, Clone, Copy, ValueEnum)]
enum Answer {
    /// Enable Context Wizard.
    On,
    /// Disable Context Wizard.
    Off,
}

fn main() -> ExitCode {
    match Cli::parse().command {
        Some(Command::Hook) => hook::run(),
        Some(Command::Decide { answer, project }) => {
            decide::run(&project, matches!(answer, Answer::On))
        }
        Some(Command::Index { project }) => index::run(&project),
        Some(Command::Status { project }) => status::run(&project),
        None => ExitCode::SUCCESS,
    }
}
