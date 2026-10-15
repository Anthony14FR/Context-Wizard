//! The `wizard` binary: hook commands and the MCP server of Context Wizard.

mod hook;

use std::process::ExitCode;

use clap::{Parser, Subcommand};

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
}

fn main() -> ExitCode {
    match Cli::parse().command {
        Some(Command::Hook) => hook::run(),
        None => ExitCode::SUCCESS,
    }
}
