//! The `purge` command, which deletes what Context Wizard stored for a project.

use std::fs;
use std::io;
use std::path::Path;
use std::process::ExitCode;

use wizard_core::{decision, settings};
use wizard_store::DATA_DIRECTORY;

use crate::install;

/// Deletes the index and the learning data of `project`, and with `all` its decision and its
/// settings override too, then says how to remove the plugin itself.
#[allow(
    clippy::print_stderr,
    reason = "the purge command reports what it could not delete"
)]
pub(crate) fn run(project: &Path, all: bool) -> ExitCode {
    let mut failed = false;
    let mut report = |what: &str, result: io::Result<bool>| match result {
        Ok(true) => println!("Deleted {what}."),
        Ok(false) => println!("Nothing to delete for {what}."),
        Err(error) => {
            failed = true;
            eprintln!(
                "Could not delete {what}: {error}. Close the other Claude Code sessions of this project and try again."
            );
        }
    };
    report(
        &format!("the index and learning data in {DATA_DIRECTORY}/"),
        remove_dir(&project.join(DATA_DIRECTORY)),
    );
    if all {
        report(
            &format!("the decision in {}", decision::RELATIVE_PATH),
            remove_file(&decision::path(project)),
        );
        if let Some(plugin_id) = install::plugin_id() {
            let _ = settings::set_tools_allowed(project, &install::tools_rule(&plugin_id), false);
            match settings::set_plugin_enabled(project, &plugin_id, true) {
                Ok(()) => println!(
                    "Removed any Context Wizard override from {}.",
                    settings::RELATIVE_PATH
                ),
                Err(error) => {
                    failed = true;
                    eprintln!("Could not update {}: {error}", settings::RELATIVE_PATH);
                }
            }
        }
        println!("Context Wizard asks again on the next prompt in this project.");
    } else {
        println!(
            "The decision in {} is kept: the index is rebuilt at the next session and learning starts over.",
            decision::RELATIVE_PATH
        );
    }
    println!(
        "To remove the plugin and its binaries everywhere, run `claude plugin uninstall context-wizard@context-wizard`."
    );
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn remove_dir(path: &Path) -> io::Result<bool> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn remove_file(path: &Path) -> io::Result<bool> {
    match fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}
