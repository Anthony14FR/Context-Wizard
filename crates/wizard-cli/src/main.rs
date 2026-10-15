//! The `wizard` binary: hook commands and the MCP server of Context Wizard.

use clap::Parser;

/// Command-line interface of the `wizard` binary.
#[derive(Debug, Parser)]
#[command(name = "wizard", version = wizard_core::VERSION, about)]
struct Cli {}

fn main() {
    Cli::parse();
}
