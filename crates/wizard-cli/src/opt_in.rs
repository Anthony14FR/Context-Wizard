//! The opt-in question injected next to the user's prompt while a project is undecided.

use std::path::Path;

/// Builds the instruction that makes Claude ask whether to enable Context Wizard.
///
/// Returns `None` when a path cannot be quoted safely: not valid UTF-8, or containing a line
/// break. The hook then stays silent.
pub(crate) fn question(binary: &Path, project: &Path) -> Option<String> {
    let binary = display(binary)?;
    let project = display(project)?;
    let command = |answer: &str| {
        format!(
            "  - bash or sh: {} decide {answer} --project {}\n  - PowerShell: & {} decide {answer} --project {}\n",
            posix_quote(&binary),
            posix_quote(&project),
            powershell_quote(&binary),
            powershell_quote(&project),
        )
    };
    Some(format!(
        "Context Wizard is installed but not configured for this project yet.\n\
         Before working on the user's request, ask the user with the AskUserQuestion tool \
         (or in plain text if that tool is unavailable, then wait for the answer): \
         \"Enable Context Wizard for this project?\", with two options:\n\
         - \"Enable\": Context Wizard indexes this project locally and gives you a compact map \
         of the codebase, so exploring it costs fewer tokens.\n\
         - \"Disable\": Context Wizard is never used in this project.\n\
         Record the answer by running the command for the shell you use.\n\
         If the user chose Enable:\n{}\
         If the user chose Disable:\n{}\
         Never run either command before the user has answered. If the user does not choose, \
         run neither. The answer is stored in .claude/wizard.json, which can be committed to \
         share the choice with the team. Then continue with the user's request.",
        command("on"),
        command("off"),
    ))
}

fn display(path: &Path) -> Option<String> {
    let text = path.to_str()?.replace('\\', "/");
    if text.contains(['\n', '\r']) {
        return None;
    }
    Some(if let Some(share) = text.strip_prefix("//?/UNC/") {
        format!("//{share}")
    } else if let Some(local) = text.strip_prefix("//?/") {
        local.to_owned()
    } else {
        text
    })
}

fn posix_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

fn powershell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{display, question};

    #[test]
    fn gives_both_shell_forms_with_forward_slashes() {
        let text = question(
            Path::new(r"C:\data\bin\wizard.exe"),
            Path::new(r"\\?\C:\work\project"),
        )
        .unwrap();
        assert!(text.contains(
            "bash or sh: 'C:/data/bin/wizard.exe' decide on --project 'C:/work/project'"
        ));
        assert!(text.contains(
            "PowerShell: & 'C:/data/bin/wizard.exe' decide off --project 'C:/work/project'"
        ));
        assert!(text.contains("Never run either command before the user has answered."));
    }

    #[test]
    fn quotes_special_characters_literally() {
        let text = question(Path::new("/opt/wizard"), Path::new("/home/o'neil/$app `x`")).unwrap();
        assert!(text.contains(r"--project '/home/o'\''neil/$app `x`'"));
        assert!(text.contains("--project '/home/o''neil/$app `x`'"));
    }

    #[test]
    fn refuses_paths_with_line_breaks() {
        assert!(question(Path::new("/opt/wizard"), Path::new("/tmp/a\nb")).is_none());
    }

    #[test]
    fn maps_verbatim_unc_paths() {
        assert_eq!(
            display(Path::new(r"\\?\UNC\server\share\p")).unwrap(),
            "//server/share/p"
        );
    }

    #[test]
    fn stays_well_under_the_hook_output_cap() {
        let long = "x".repeat(500);
        let text = question(Path::new(&long), Path::new(&long)).unwrap();
        assert!(text.len() < 10_000);
    }
}
