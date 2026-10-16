//! The `stats` command, which reports what Context Wizard learned about a project.

use std::fmt::Write as _;
use std::path::Path;
use std::process::ExitCode;

use wizard_learn::{LEARN_FILE, Learn, ROLLING_WINDOW};
use wizard_store::DATA_DIRECTORY;

const TOP_FILES: usize = 10;
const GLOSSARY_EXAMPLES: usize = 5;
const BAR_WIDTH: f64 = 20.0;

/// Prints the precision curve, the weight of the code structure, the most useful files and the
/// glossary of `project`.
pub(crate) fn run(project: &Path) -> ExitCode {
    print!("{}", report(project));
    ExitCode::SUCCESS
}

fn report(project: &Path) -> String {
    let mut text = format!("Context Wizard learning for {}\n", project.display());
    if !project.join(DATA_DIRECTORY).join(LEARN_FILE).is_file() {
        text.push_str("No session recorded yet. Learning starts once the project is enabled.\n");
        return text;
    }
    let learn = match Learn::open_project(project) {
        Ok(learn) => learn,
        Err(error) => {
            let _ = writeln!(text, "The learning store cannot be read: {error}");
            return text;
        }
    };
    let sessions = learn.sessions().map_or(0, |sessions| sessions.len());
    let evaluations = learn.evaluations(ROLLING_WINDOW).unwrap_or_default();
    let _ = writeln!(text, "Sessions recorded: {sessions}");
    match learn.rolling_precision().ok().flatten() {
        Some(precision) => {
            let _ = writeln!(
                text,
                "Precision@5 over the last {} graded sessions: {precision:.2}",
                evaluations.len()
            );
        }
        None => {
            text.push_str(
                "Precision@5: no graded session yet (a session is graded when it edits files)\n",
            );
        }
    }
    if let Ok(alpha) = learn.alpha() {
        let _ = writeln!(
            text,
            "Weight of the code structure in predictions (alpha): {alpha:.2}"
        );
    }
    if !evaluations.is_empty() {
        text.push_str("\nPrecision@5 per graded session, oldest first:\n");
        for evaluation in &evaluations {
            let _ = writeln!(
                text,
                "  {:.2} {}",
                evaluation.precision,
                bar(evaluation.precision)
            );
        }
    }
    let mut signals = learn.file_signals().unwrap_or_default();
    signals.retain(|signal| signal.useful > 0 || signal.is_pivot());
    signals.sort_by(|left, right| {
        right
            .useful
            .cmp(&left.useful)
            .then(right.read_sessions.cmp(&left.read_sessions))
            .then(left.path.cmp(&right.path))
    });
    if !signals.is_empty() {
        text.push_str("\nMost useful files (sessions that read then edited them):\n");
        for signal in signals.iter().take(TOP_FILES) {
            let pivot = if signal.is_pivot() { ", pivot" } else { "" };
            let _ = writeln!(
                text,
                "  - {} ({} useful, read in {} sessions{pivot})",
                signal.path, signal.useful, signal.read_sessions
            );
        }
    }
    let glossary = learn.glossary().unwrap_or_default();
    if !glossary.is_empty() {
        let _ = writeln!(text, "\nGlossary: {} words linked to files", glossary.len());
        let mut strongest = glossary;
        strongest.sort_by(|left, right| {
            let weight =
                |entry: &wizard_learn::GlossaryEntry| entry.files.first().map_or(0, |file| file.1);
            weight(right)
                .cmp(&weight(left))
                .then(left.term.cmp(&right.term))
        });
        for entry in strongest.iter().take(GLOSSARY_EXAMPLES) {
            if let Some((file, sessions)) = entry.files.first() {
                let _ = writeln!(text, "  - {} -> {file} ({sessions} sessions)", entry.term);
            }
        }
    }
    text
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the value is clamped between 0 and the bar width"
)]
fn bar(precision: f64) -> String {
    "#".repeat((precision.clamp(0.0, 1.0) * BAR_WIDTH).round() as usize)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use wizard_learn::{Event, EventKind, Learn, StoredPrediction};

    use super::report;

    #[test]
    fn reports_an_empty_project_without_creating_anything() {
        let project = tempfile::tempdir().unwrap();
        assert!(report(project.path()).contains("No session recorded yet"));
        assert!(!project.path().join(".wizard").exists());
    }

    #[test]
    fn reports_precision_alpha_and_useful_files() {
        let project = tempfile::tempdir().unwrap();
        let mut learn = Learn::open_project(project.path()).unwrap();
        learn
            .record_prediction(&StoredPrediction {
                session: "s".to_owned(),
                at: 1,
                files: vec!["src/a.rs".to_owned(), "src/b.rs".to_owned()],
                confidence: 0.5,
                alpha: 0.7,
                shown: true,
            })
            .unwrap();
        for kind in [EventKind::Read, EventKind::Edit] {
            learn
                .record(&Event {
                    path: Some("src/a.rs".to_owned()),
                    ..Event::new("s", 2, kind)
                })
                .unwrap();
        }
        learn.consolidate("s").unwrap();
        learn.evaluate("s", 3).unwrap();
        let text = report(project.path());
        assert!(text.contains("Sessions recorded: 1"), "{text}");
        assert!(
            text.contains("over the last 1 graded sessions: 0.50"),
            "{text}"
        );
        assert!(text.contains("(alpha): 0.70"), "{text}");
        assert!(text.contains("  0.50 ##########\n"), "{text}");
        assert!(
            text.contains("  - src/a.rs (1 useful, read in 1 sessions)"),
            "{text}"
        );
    }
}
