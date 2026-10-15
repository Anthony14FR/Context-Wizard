//! The `decide` command, which records the opt-in answer of a project.

use std::path::Path;
use std::process::ExitCode;

use wizard_core::date::Date;
use wizard_core::decision;

/// Records whether Context Wizard is enabled for `project` and reports the result.
#[allow(
    clippy::print_stderr,
    reason = "the decide command reports failures to the user"
)]
pub(crate) fn run(project: &Path, enabled: bool) -> ExitCode {
    match decision::decide(project, enabled, Date::today()) {
        Ok(_) => {
            let state = if enabled { "enabled" } else { "disabled" };
            println!(
                "Context Wizard is {state} for {}. The choice is stored in {}.",
                project.display(),
                decision::RELATIVE_PATH
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("Context Wizard could not record the decision: {error}");
            ExitCode::FAILURE
        }
    }
}
