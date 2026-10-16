//! Prediction of the files a prompt needs, injected as context when it is confident enough.
//!
//! Every prediction is recorded, shown or not, so the session's `Stop` can grade it.

use std::fmt::Write as _;
use std::path::Path;

use wizard_core::text::keywords;
use wizard_index::predict::{PredictInput, PredictOptions, Prediction, predict};
use wizard_index::structure::Structure;
use wizard_learn::{Learn, StoredPrediction, now};
use wizard_store::Index;

use crate::event::Event;

/// Predicts the files of the prompt in `event` and returns the context to inject, if any.
pub(crate) fn inject(project: &Path, event: &Event) -> Option<String> {
    let prompt = event.prompt.as_deref()?.trim();
    if prompt.is_empty() || prompt.starts_with('/') {
        return None;
    }
    let mut index = Index::open_project(project).ok()?;
    let structure = Structure::load(&mut index).ok()?;
    if structure.paths.is_empty() {
        return None;
    }
    let matches = index
        .definitions_named(&structure.names_matching(&keywords(prompt)))
        .ok()?;
    let mut learn = Learn::open_project(project).ok();
    let session = event.session_id.as_deref().filter(|id| !id.is_empty());
    let options = PredictOptions::default();
    let at = now();
    let prediction = predict(
        &PredictInput {
            structure: &structure,
            matches: &matches,
            learn: learn.as_ref(),
            prompt,
            session,
            now: at,
        },
        &options,
    );
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

fn render(prediction: &Prediction) -> String {
    let mut text = format!(
        "Context Wizard predicts these files for this task (confidence {:.2}). Start with them, \
         and use the context-wizard MCP tools to look further:\n",
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
