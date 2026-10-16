//! The `recipes` command, which shows the recipes of a project and the problems of broken ones.

use std::fmt::Write as _;
use std::path::Path;
use std::process::ExitCode;

use wizard_index::recipes::Applied;
use wizard_recipes::PROJECT_RECIPES;

/// Prints the loaded recipes, where each one applies and every loading problem, and fails when
/// a recipe is broken so authors can use it as a check.
#[allow(
    clippy::print_stderr,
    reason = "the recipes command reports broken recipes to their author"
)]
pub(crate) fn run(project: &Path) -> ExitCode {
    let applied = Applied::detect_project(project);
    print!("{}", report(project, &applied));
    if applied.errors.is_empty() {
        ExitCode::SUCCESS
    } else {
        for error in &applied.errors {
            eprintln!("{error}");
        }
        ExitCode::FAILURE
    }
}

fn report(project: &Path, applied: &Applied) -> String {
    let mut text = format!(
        "Recipes for {} (project recipes go in {PROJECT_RECIPES}/<name>/recipe.toml)\n",
        project.display()
    );
    for (name, extracts) in applied.loaded() {
        let roots: Vec<String> = applied
            .detected()
            .into_iter()
            .filter(|(detected, _)| *detected == name)
            .map(|(_, root)| root)
            .collect();
        let state = if roots.is_empty() {
            "not detected".to_owned()
        } else {
            format!("applies to {}", roots.join(", "))
        };
        let _ = writeln!(text, "- {name}: {state}; extracts {}", extracts.join(", "));
    }
    if !applied.errors.is_empty() {
        let _ = writeln!(
            text,
            "{} problem(s) found, listed on standard error.",
            applied.errors.len()
        );
    }
    text
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::fs;

    use wizard_index::recipes::Applied;
    use wizard_recipes::PROJECT_RECIPES;

    use super::report;

    #[test]
    fn lists_recipes_where_they_apply_and_counts_problems() {
        let project = tempfile::tempdir().unwrap();
        fs::write(
            project.path().join("package.json"),
            r#"{"dependencies":{"react":"19"}}"#,
        )
        .unwrap();
        let broken = project.path().join(PROJECT_RECIPES).join("broken");
        fs::create_dir_all(&broken).unwrap();
        fs::write(broken.join("recipe.toml"), "name = \"broken\"\n").unwrap();
        let applied = Applied::detect_project(project.path());
        let text = report(project.path(), &applied);
        assert!(
            text.contains("- react: applies to .; extracts components"),
            "{text}"
        );
        assert!(text.contains("- laravel: not detected;"), "{text}");
        assert!(text.contains("1 problem(s) found"), "{text}");
        assert!(
            applied.errors[0]
                .to_string()
                .contains("missing field `detect`")
        );
    }
}
