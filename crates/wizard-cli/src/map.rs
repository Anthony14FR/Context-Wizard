//! The `map` command, which prints the L0 overview or the L1 map of an indexed project.

use std::path::Path;
use std::process::ExitCode;

use wizard_index::calibration;
use wizard_index::map::{L1_BUDGET, map_with};
use wizard_index::overview::overview;
use wizard_store::Index;

/// Prints the overview (level 0) or the map (level 1) of `project` from its current index.
#[allow(
    clippy::print_stderr,
    reason = "the map command reports failures to the user"
)]
pub(crate) fn run(project: &Path, level: u8) -> ExitCode {
    let rendered = Index::open_project(project).and_then(|index| match level {
        0 => overview(project, &index),
        _ => map_with(&index, L1_BUDGET, &calibration::estimator(project)),
    });
    match rendered {
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
