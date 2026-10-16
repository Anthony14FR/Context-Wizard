//! The `decide` command, which records the opt-in answer of a project.

use std::path::Path;
use std::process::ExitCode;

use wizard_core::date::Date;
use wizard_core::{decision, settings};

use crate::{context, install};

/// Records whether Context Wizard is enabled for `project` and reports the result.
#[allow(
    clippy::print_stderr,
    reason = "the decide command reports failures to the user"
)]
pub(crate) fn run(project: &Path, enabled: bool) -> ExitCode {
    if let Err(error) = decision::decide(project, enabled, Date::today()) {
        eprintln!("Context Wizard could not record the decision: {error}");
        return ExitCode::FAILURE;
    }
    let state = if enabled { "enabled" } else { "disabled" };
    println!(
        "Context Wizard is {state} for {}. The choice is stored in {}.",
        project.display(),
        decision::RELATIVE_PATH
    );
    if enabled && let Some(text) = context::refreshed_overview(project) {
        println!(
            "
{text}"
        );
    }
    let Some(plugin_id) = install::plugin_id() else {
        if !enabled {
            eprintln!(
                "Context Wizard is not running from a Claude Code plugin install, so {} was \
                 left untouched. It stays inert in this project.",
                settings::RELATIVE_PATH
            );
        }
        return ExitCode::SUCCESS;
    };
    match settings::set_plugin_enabled(project, &plugin_id, enabled) {
        Ok(()) if !enabled => println!(
            "Claude Code stops loading Context Wizard in this project from the next session. \
             The override is in {}.",
            settings::RELATIVE_PATH
        ),
        Ok(()) => {}
        Err(error) => eprintln!(
            "Context Wizard could not update {}: {error}. It stays inert in this project.",
            settings::RELATIVE_PATH
        ),
    }
    ExitCode::SUCCESS
}
