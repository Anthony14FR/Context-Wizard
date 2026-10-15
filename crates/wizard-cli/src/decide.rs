//! The `decide` command, which records the opt-in answer of a project.

use std::env;
use std::path::Path;
use std::process::ExitCode;

use wizard_core::date::Date;
use wizard_core::{decision, settings};

const PLUGIN_NAME: &str = "context-wizard";

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
    let Some(plugin_id) = plugin_id() else {
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

/// Returns the `<plugin>@<marketplace>` id of the installed plugin, or `None` when the binary
/// does not run from a plugin data directory.
///
/// Claude Code stores plugin data in `<plugin>-<marketplace>`, and the binary lives in its
/// `bin` directory, so the marketplace is read from that directory name.
fn plugin_id() -> Option<String> {
    let binary = env::current_exe().ok()?;
    let binary = binary.canonicalize().unwrap_or(binary);
    let bin = binary.parent()?;
    if bin.file_name()? != "bin" {
        return None;
    }
    let name = bin.parent()?.file_name()?.to_str()?;
    let marketplace = marketplace_from_data_dir(name)?;
    Some(format!("{PLUGIN_NAME}@{marketplace}"))
}

fn marketplace_from_data_dir(name: &str) -> Option<&str> {
    name.strip_prefix(PLUGIN_NAME)?
        .strip_prefix('-')
        .filter(|marketplace| !marketplace.is_empty())
}

#[cfg(test)]
mod tests {
    use super::marketplace_from_data_dir;

    #[test]
    fn reads_the_marketplace_from_the_data_directory() {
        assert_eq!(
            marketplace_from_data_dir("context-wizard-context-wizard"),
            Some("context-wizard")
        );
        assert_eq!(
            marketplace_from_data_dir("context-wizard-context-wizard-beta"),
            Some("context-wizard-beta")
        );
        assert_eq!(
            marketplace_from_data_dir("context-wizard-inline"),
            Some("inline")
        );
        assert_eq!(marketplace_from_data_dir("debug"), None);
        assert_eq!(marketplace_from_data_dir("context-wizard"), None);
    }
}
