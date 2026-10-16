//! The `wizard` binary: hook commands and the MCP server of Context Wizard.

mod calibrate;
mod context;
mod decide;
mod event;
mod grep;
mod hook;
mod index;
mod install;
mod learn;
mod map;
mod opt_in;
mod prediction;
mod purge;
mod recipes;
mod serve;
mod stats;
mod status;
mod update;

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
    /// Measures the token ratios of a project with Anthropic's free `count_tokens` endpoint,
    /// using the key in `ANTHROPIC_API_KEY`.
    Calibrate {
        /// Model whose tokenizer to measure.
        #[arg(long, default_value = "claude-sonnet-5-5")]
        model: String,
        /// Project root. Defaults to the current directory.
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
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
    /// Prints the overview or the map of an indexed project.
    Map {
        /// 0 for the overview, 1 for the map of definitions.
        #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(u8).range(0..=1))]
        level: u8,
        /// Project root. Defaults to the current directory.
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
    /// Runs the MCP server of a project over standard input and output.
    Serve {
        /// Project root. Defaults to `CLAUDE_PROJECT_DIR`, then the current directory.
        #[arg(long)]
        project: Option<PathBuf>,
    },
    /// Deletes the index and learning data of a project, and with `--all` its decision too.
    Purge {
        /// Also delete the decision and the settings override.
        #[arg(long)]
        all: bool,
        /// Project root. Defaults to the current directory.
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
    /// Prints the files predicted for a prompt as JSON, without recording anything.
    #[command(hide = true)]
    Predict {
        /// The prompt to predict files for.
        prompt: String,
        /// Ignore what past sessions taught.
        #[arg(long)]
        no_learning: bool,
        /// Project root. Defaults to the current directory.
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
    /// Lists the framework recipes of a project and checks them, failing on a broken recipe.
    Recipes {
        /// Project root. Defaults to the current directory.
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
    /// Shows what Context Wizard learned about a project and how well it predicts.
    Stats {
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
    /// Records the latest released version, for the update notice of later sessions.
    #[command(hide = true)]
    UpdateCheck,
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
        Some(Command::Calibrate { model, project }) => calibrate::run(&project, &model),
        Some(Command::Decide { answer, project }) => {
            decide::run(&project, matches!(answer, Answer::On))
        }
        Some(Command::Index { project }) => index::run(&project),
        Some(Command::Map { level, project }) => map::run(&project, level),
        Some(Command::Serve { project }) => serve::run(project),
        Some(Command::Purge { all, project }) => purge::run(&project, all),
        Some(Command::Predict {
            prompt,
            no_learning,
            project,
        }) => prediction::run(&project, &prompt, !no_learning),
        Some(Command::Recipes { project }) => recipes::run(&project),
        Some(Command::Stats { project }) => stats::run(&project),
        Some(Command::Status { project }) => status::run(&project),
        Some(Command::UpdateCheck) => update::check(),
        None => ExitCode::SUCCESS,
    }
}
