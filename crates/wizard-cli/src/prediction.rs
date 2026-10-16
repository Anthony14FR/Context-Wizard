//! Prediction of the files a prompt needs, injected as context when it is confident enough.
//!
//! Every prediction is recorded, shown or not, so the session's `Stop` can grade it.

use std::fmt::Write as _;
use std::path::Path;

use wizard_core::text::stems;
use wizard_index::predict::{DEFAULT_ALPHA, PredictInput, PredictOptions, Prediction, predict};
use wizard_index::structure::Structure;
use wizard_index::vocabulary::Vocabulary;
use wizard_learn::{Learn, StoredPrediction, now};
use wizard_store::Index;

use crate::event::Event;

/// Predicts the files of the prompt in `event` and returns the context to inject, if any.
pub(crate) fn inject(project: &Path, event: &Event) -> Option<String> {
    let prompt = event.prompt.as_deref()?.trim();
    if prompt.is_empty() || prompt.starts_with('/') {
        return None;
    }
    let session = event.session_id.as_deref().filter(|id| !id.is_empty());
    let mut learn = Learn::open_project(project).ok();
    let at = now();
    let (prediction, options) = compute(project, prompt, session, learn.as_ref(), at)?;
    let shown = prediction.is_confident();
    if let (Some(learn), Some(session)) = (learn.as_mut(), session) {
        let _ = learn.record_prediction(&StoredPrediction {
            session: session.to_owned(),
            at,
            files: prediction
                .files
                .iter()
                .map(|file| file.path.clone())
                .collect(),
            confidence: prediction.confidence,
            alpha: options.alpha,
            shown,
        });
    }
    shown.then(|| render(&prediction))
}

/// Predicts the files of `prompt` in `project`, with learning from `learn` when given.
pub(crate) fn compute(
    project: &Path,
    prompt: &str,
    session: Option<&str>,
    learn: Option<&Learn>,
    at: i64,
) -> Option<(Prediction, PredictOptions)> {
    let mut index = Index::open_project(project).ok()?;
    let structure = Structure::load(&mut index).ok()?;
    if structure.paths.is_empty() {
        return None;
    }
    let vocabulary = Vocabulary::load(&mut index, &structure.paths).ok()?;
    let words = stems(prompt);
    let related: Vec<&str> = structure
        .names
        .iter()
        .filter(|name| Vocabulary::relates(name, &words))
        .map(String::as_str)
        .collect();
    let matches = index.definitions_named(&related).ok()?;
    let options = PredictOptions {
        alpha: learn
            .and_then(|learn| learn.alpha().ok())
            .unwrap_or(DEFAULT_ALPHA),
        ..PredictOptions::default()
    };
    let prediction = predict(
        &PredictInput {
            structure: &structure,
            vocabulary: &vocabulary,
            matches: &matches,
            learn,
            prompt,
            session,
            now: at,
        },
        &options,
    );
    Some((prediction, options))
}

/// Prints the prediction for `prompt` as JSON, without recording it, for evaluations.
#[allow(
    clippy::print_stdout,
    reason = "the predict command prints its result for evaluation scripts"
)]
pub(crate) fn run(project: &Path, prompt: &str, learning: bool) -> std::process::ExitCode {
    let learn = learning
        .then(|| Learn::open_project(project).ok())
        .flatten();
    let Some((prediction, options)) = compute(project, prompt, None, learn.as_ref(), now()) else {
        return std::process::ExitCode::FAILURE;
    };
    let files: Vec<serde_json::Value> = prediction
        .files
        .iter()
        .map(|file| {
            serde_json::json!({
                "path": file.path,
                "score": file.score,
                "lines": file.lines,
                "explored": file.explored,
            })
        })
        .collect();
    println!(
        "{}",
        serde_json::json!({
            "confidence": prediction.confidence,
            "shown": prediction.is_confident(),
            "alpha": options.alpha,
            "files": files,
        })
    );
    std::process::ExitCode::SUCCESS
}

fn render(prediction: &Prediction) -> String {
    let mut text = format!(
        "Files likely involved in this task, predicted by Context Wizard (confidence {:.2}). \
         Check them first; `explain` gives a file's outline without reading it:\n",
        prediction.confidence
    );
    for file in &prediction.files {
        let _ = write!(text, "- {}", file.path);
        if !file.lines.is_empty() {
            let ranges: Vec<String> = file
                .lines
                .iter()
                .map(|(start, end)| {
                    if start == end {
                        start.to_string()
                    } else {
                        format!("{start}-{end}")
                    }
                })
                .collect();
            let _ = write!(text, " (lines {})", ranges.join(", "));
        }
        text.push('\n');
    }
    text
}

#[cfg(test)]
mod tests {
    use wizard_index::predict::{PredictedFile, Prediction};

    use super::render;

    #[test]
    fn renders_files_with_their_line_ranges() {
        let prediction = Prediction {
            files: vec![
                PredictedFile {
                    path: "src/invoice.rs".to_owned(),
                    score: 0.9,
                    lines: vec![(3, 7), (12, 12)],
                    explored: false,
                },
                PredictedFile {
                    path: "src/tax.rs".to_owned(),
                    score: 0.4,
                    lines: Vec::new(),
                    explored: true,
                },
            ],
            confidence: 0.65,
        };
        let text = render(&prediction);
        assert!(text.contains("(confidence 0.65)"), "{text}");
        assert!(
            text.contains("- src/invoice.rs (lines 3-7, 12)\n- src/tax.rs\n"),
            "{text}"
        );
    }
}
