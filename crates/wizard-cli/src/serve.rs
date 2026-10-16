//! The `serve` command, which runs the MCP server of a project over standard input and output.

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use crate::{context, install};

/// Runs the MCP server until the client disconnects.
///
/// The project is `project`, or `CLAUDE_PROJECT_DIR`, or the current directory. A binary whose
/// version differs from the plugin's exits at once, so the next session starts the right one.
#[allow(
    clippy::print_stderr,
    reason = "MCP servers report startup failures on standard error"
)]
pub(crate) fn run(project: Option<PathBuf>) -> ExitCode {
    if install::version_mismatch() {
        eprintln!("Context Wizard is updating; the server starts with the next session.");
        return ExitCode::FAILURE;
    }
    let project = project
        .or_else(|| {
            env::var_os("CLAUDE_PROJECT_DIR")
                .filter(|dir| !dir.is_empty())
                .map(PathBuf::from)
        })
        .or_else(|| env::current_dir().ok());
    let Some(project) = project else {
        eprintln!("Context Wizard could not determine the project directory.");
        return ExitCode::FAILURE;
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("Context Wizard could not start its runtime: {error}");
            return ExitCode::FAILURE;
        }
    };
    match runtime.block_on(wizard_mcp::serve_stdio(project, context::build_options())) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Context Wizard server stopped: {error}");
            ExitCode::FAILURE
        }
    }
}
