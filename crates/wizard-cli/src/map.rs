//! The `map` command, which prints the L0 overview of an indexed project.

use std::path::Path;
use std::process::ExitCode;

use wizard_index::overview::overview;
use wizard_store::Index;

/// Prints the overview of `project` from its current index.
#[allow(
    clippy::print_stderr,
    reason = "the map command reports failures to the user"
)]
pub(crate) fn run(project: &Path) -> ExitCode {
    match Index::open_project(project).and_then(|index| overview(project, &index)) {
        Ok(text) => {
            print!("{text}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!(
                "Context Wizard could not read the index of {}: {error}",
                project.display()
            );
            ExitCode::FAILURE
        }
    }
}
