//! The `status` command, which reports the state of Context Wizard for a project.

use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

use wizard_core::decision::{self, State};
use wizard_core::{VERSION, settings};

/// Prints the decision, the settings override, the binary location and the last install error.
pub(crate) fn run(project: &Path) -> ExitCode {
    print!("{}", report(project));
    ExitCode::SUCCESS
}

fn report(project: &Path) -> String {
    let mut text = format!(
        "Context Wizard {VERSION}\nProject: {}\nDecision: {}\n",
        project.display(),
        describe_decision(project)
    );
    let override_present = fs::read_to_string(project.join(settings::RELATIVE_PATH))
        .is_ok_and(|content| content.contains("\"context-wizard@"));
    let _ = writeln!(
        text,
        "Claude Code override in {}: {}",
        settings::RELATIVE_PATH,
        if override_present { "present" } else { "none" }
    );
    match env::current_exe() {
        Ok(binary) => {
            let _ = writeln!(text, "Binary: {}", binary.display());
            let error = binary
                .parent()
                .and_then(|bin| fs::read_to_string(bin.join("install-error")).ok());
            let _ = writeln!(
                text,
                "Last install error: {}",
                error.as_deref().map_or("none", str::trim)
            );
        }
        Err(error) => {
            let _ = writeln!(text, "Binary: unknown ({error})");
        }
    }
    text
}

fn describe_decision(project: &Path) -> String {
    match decision::load(project) {
        Ok(State::Undecided) => format!(
            "not decided yet, Claude asks on the next prompt ({} is absent)",
            decision::RELATIVE_PATH
        ),
        Ok(State::Decided(decision)) if decision.enabled => {
            format!("enabled on {}", decision.decided_at)
        }
        Ok(State::Decided(decision)) => format!("disabled on {}", decision.decided_at),
        Ok(State::Unsupported { version }) => format!(
            "written by a newer Context Wizard (schema {version}), inert until the plugin is updated"
        ),
        Err(error) => format!("unreadable, inert until fixed: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use wizard_core::date::Date;
    use wizard_core::decision;

    use super::report;

    #[test]
    fn reports_each_decision_state() {
        let project = tempfile::tempdir().unwrap();
        assert!(report(project.path()).contains("Decision: not decided yet"));
        decision::decide(project.path(), true, Date::from_unix_days(20_741)).unwrap();
        let text = report(project.path());
        assert!(text.contains("Decision: enabled on 2026-10-15"));
        assert!(text.contains("override in .claude/settings.local.json: none"));
        assert!(text.contains("Last install error:"));
    }
}
