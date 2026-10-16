//! The `index` command, which builds or updates the index of a project.

use std::path::Path;
use std::process::ExitCode;

use wizard_core::decision;
use wizard_index::build::build;

use crate::context;

/// Builds the index of `project` and prints a one-line summary.
#[allow(
    clippy::print_stderr,
    reason = "the index command reports failures to the user"
)]
pub(crate) fn run(project: &Path) -> ExitCode {
    if !decision::load(project).is_ok_and(|state| state.is_enabled()) {
        eprintln!(
            "Context Wizard is not enabled for {}. Run `wizard decide on` first.",
            project.display()
        );
        return ExitCode::FAILURE;
    }
    match build(project, &context::build_options()) {
        Ok(report) => {
            println!(
                "Indexed {} files in {:.1?}: {} added, {} modified, {} removed, {} parsed, {} lexical.",
                report.files,
                report.elapsed,
                report.changes.added.len(),
                report.changes.modified.len(),
                report.changes.removed.len(),
                report.tagged,
                report.lexical
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!(
                "Context Wizard could not index {}: {error}",
                project.display()
            );
            ExitCode::FAILURE
        }
    }
}
