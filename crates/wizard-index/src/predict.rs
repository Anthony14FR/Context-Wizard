//! Prediction of the files a task needs.
//!
//! Each file gets `score = α · structural + (1 − α) · learned`. The structural part is the lift
//! of a personalised `PageRank` seeded by the files whose words best match the task (BM25 over
//! stemmed words) and by learned files, with tests and documentation ranked after production
//! code unless the task is about them. The learned part
//! combines the files of similar past sessions, the domain glossary and each file's own history
//! of useful and noisy reads, all faded with a 30-day half-life. Both parts are scaled to 1 for
//! the best file, and α stays between 0.5 and 0.9 so the structure of the code always counts.
//!
//! One prediction in ten swaps its last file for the best structural candidate that learning
//! knows nothing about, so files never opened before get a chance to be learned.
//!
//! Confidence is the share of the task's rare words that the predicted files hold, blended with
//! the learned scores. On the benchmark tasks, predictions under 0.2 were never right, and the
//! threshold of 0.55 keeps the shown ones to those where about half the files are right.

use std::collections::BTreeMap;
use std::hash::{DefaultHasher, Hash, Hasher};

use wizard_core::text::{keywords, stems};
pub use wizard_learn::{DEFAULT_ALPHA, MAX_ALPHA, MIN_ALPHA};
use wizard_learn::{FileSignal, Learn, decay};
use wizard_store::LocatedSymbol;

use crate::learned::Learned;
use crate::query::penalise_tests;
use crate::structure::Structure;
use crate::vocabulary::Vocabulary;

/// Share of predictions that explore a file learning knows nothing about.
pub const EXPLORATION: f64 = 0.1;
/// Confidence below which a prediction is not shown.
pub const CONFIDENCE_THRESHOLD: f64 = 0.55;
/// Number of files a prediction holds.
pub const PREDICTED_FILES: usize = 5;

const TASK_SHARE: f64 = 0.5;
const GLOSSARY_SHARE: f64 = 0.3;
const HISTORY_SHARE: f64 = 0.2;
const NOISE_PENALTY: f64 = 0.2;
const PIVOT_BONUS: f64 = 0.5;
const LEARNED_SEED_WEIGHT: f64 = 2.0;
const MAX_LINES: usize = 3;
const MAX_SEEDS: usize = 30;

/// Settings of a prediction.
#[derive(Debug, Clone, PartialEq)]
pub struct PredictOptions {
    /// Weight of the structural part, kept between [`MIN_ALPHA`] and [`MAX_ALPHA`].
    pub alpha: f64,
    /// Number of files to predict.
    pub limit: usize,
    /// Share of predictions that explore, between 0 and 1.
    pub exploration: f64,
}

impl Default for PredictOptions {
    fn default() -> Self {
        Self {
            alpha: DEFAULT_ALPHA,
            limit: PREDICTED_FILES,
            exploration: EXPLORATION,
        }
    }
}

/// A file predicted for a task.
#[derive(Debug, Clone, PartialEq)]
pub struct PredictedFile {
    /// File, relative to the project root.
    pub path: String,
    /// Combined score, between 0 and 1.
    pub score: f64,
    /// First and last lines of the narrowest definitions matching the task, at most three.
    pub lines: Vec<(u32, u32)>,
    /// Whether the file was chosen to explore rather than for its score.
    pub explored: bool,
}

/// Files predicted for a task.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Prediction {
    /// Predicted files, best first.
    pub files: Vec<PredictedFile>,
    /// How likely the prediction is right, between 0 and 1.
    pub confidence: f64,
}

impl Prediction {
    /// Returns `true` when the prediction is confident enough to be shown.
    #[must_use]
    pub fn is_confident(&self) -> bool {
        !self.files.is_empty() && self.confidence >= CONFIDENCE_THRESHOLD
    }
}

/// What a prediction is computed from.
#[derive(Debug, Clone, Copy)]
pub struct PredictInput<'a> {
    /// File graph of the project.
    pub structure: &'a Structure,
    /// Words of each file of the structure.
    pub vocabulary: &'a Vocabulary,
    /// Definitions whose name contains a keyword of the prompt.
    pub matches: &'a [LocatedSymbol],
    /// Learning store of the project, when it has one.
    pub learn: Option<&'a Learn>,
    /// The task, as the user wrote it.
    pub prompt: &'a str,
    /// Current session, left out of the past sessions.
    pub session: Option<&'a str>,
    /// Current time in milliseconds since the Unix epoch.
    pub now: i64,
}

/// Predicts the files the task of `input` needs.
#[must_use]
pub fn predict(input: &PredictInput<'_>, options: &PredictOptions) -> Prediction {
    let alpha = options.alpha.clamp(MIN_ALPHA, MAX_ALPHA);
    let learned_files = input
        .learn
        .map(|learn| learned_scores(learn, input.prompt, input.session, input.now))
        .unwrap_or_default();
    let learned_graph = input
        .learn
        .and_then(|learn| Learned::from_store(learn, input.now))
        .unwrap_or_default();
    let structure = input.structure;
    let keywords = keywords(input.prompt);
    let words = stems(input.prompt);
    let mut seeds = word_seeds(input.vocabulary, input.prompt);
    let mut matches: BTreeMap<usize, Vec<&LocatedSymbol>> = BTreeMap::new();
    for located in input.matches {
        if let Some(node) = structure.position(&located.path)
            && Vocabulary::relates(&located.symbol.name, &words)
        {
            matches.entry(node).or_default().push(located);
        }
    }
    for (path, score) in &learned_files {
        if let Some(node) = structure.position(path) {
            *seeds.entry(node).or_default() += LEARNED_SEED_WEIGHT * score;
        }
    }
    if seeds.is_empty() && learned_files.is_empty() {
        return Prediction::default();
    }
    let structural: Vec<f64> = if seeds.is_empty() {
        vec![0.0; structure.paths.len()]
    } else {
        let mut relevance = structure.relevance(&seeds, &learned_graph.co_access);
        penalise_tests(&structure.paths, &mut relevance, &keywords);
        scaled(&relevance)
    };
    let mut scores: BTreeMap<&str, (f64, f64)> = BTreeMap::new();
    for (node, path) in structure.paths.iter().enumerate() {
        if structural[node] > 0.0 {
            scores.entry(path).or_default().0 = structural[node];
        }
    }
    for (path, score) in &learned_files {
        scores.entry(path).or_default().1 = *score;
    }
    let mut ranked: Vec<(&str, f64, f64)> = scores
        .into_iter()
        .map(|(path, (structural, learned))| {
            (path, alpha * structural + (1.0 - alpha) * learned, learned)
        })
        .collect();
    ranked.sort_by(|left, right| right.1.total_cmp(&left.1).then(left.0.cmp(right.0)));
    let mut chosen: Vec<(&str, f64, bool)> = ranked
        .iter()
        .take(options.limit)
        .map(|(path, score, _)| (*path, *score, false))
        .collect();
    if chosen.len() == options.limit
        && explores(input.prompt, input.session, options.exploration)
        && let Some((path, score, _)) = ranked
            .iter()
            .skip(options.limit)
            .find(|(_, _, learned)| *learned == 0.0)
        && let Some(last) = chosen.last_mut()
    {
        *last = (path, *score, true);
    }
    let files: Vec<PredictedFile> = chosen
        .into_iter()
        .map(|(path, score, explored)| PredictedFile {
            path: path.to_owned(),
            score,
            lines: structure
                .position(path)
                .and_then(|node| matches.get(&node))
                .map(|found| narrowest_ranges(found))
                .unwrap_or_default(),
            explored,
        })
        .collect();
    let nodes: Vec<usize> = files
        .iter()
        .filter_map(|file| structure.position(&file.path))
        .collect();
    let coverage = input.vocabulary.coverage(input.prompt, &nodes);
    let learned = mean(
        files
            .iter()
            .filter_map(|file| learned_files.get(&file.path).copied()),
    );
    let confidence = match coverage {
        Some(coverage) if learned_files.is_empty() => coverage,
        Some(coverage) => alpha * coverage + (1.0 - alpha) * learned,
        None => learned,
    };
    Prediction { files, confidence }
}

/// Seeds of the ranking: the files whose words best match the task, scaled so the best is 1.
fn word_seeds(vocabulary: &Vocabulary, prompt: &str) -> BTreeMap<usize, f64> {
    let mut scored: Vec<(usize, f64)> = vocabulary.scores(prompt).into_iter().collect();
    scored.sort_by(|left, right| right.1.total_cmp(&left.1).then(left.0.cmp(&right.0)));
    scored.truncate(MAX_SEEDS);
    let top = scored.first().map_or(0.0, |(_, score)| *score);
    if top <= 0.0 {
        return BTreeMap::new();
    }
    scored
        .into_iter()
        .map(|(node, score)| (node, score / top))
        .collect()
}

fn narrowest_ranges(found: &[&LocatedSymbol]) -> Vec<(u32, u32)> {
    let mut ranges: Vec<(u32, u32)> = found
        .iter()
        .map(|located| (located.symbol.line, located.symbol.end_line))
        .collect();
    ranges.sort_by_key(|(start, end)| (end.saturating_sub(*start), *start));
    ranges.dedup();
    ranges.truncate(MAX_LINES);
    ranges.sort_unstable();
    ranges
}

fn learned_scores(
    learn: &Learn,
    prompt: &str,
    session: Option<&str>,
    now: i64,
) -> BTreeMap<String, f64> {
    let tasks: BTreeMap<String, f64> = learn
        .task_files(prompt, session, now)
        .unwrap_or_default()
        .into_iter()
        .map(|file| (file.path, file.score))
        .collect();
    let glossary: BTreeMap<String, f64> = learn
        .glossary_files(prompt)
        .unwrap_or_default()
        .into_iter()
        .map(|(path, sessions)| (path, f64::from(sessions)))
        .collect();
    let relevant = |path: &str| tasks.contains_key(path) || glossary.contains_key(path);
    let history: BTreeMap<String, f64> = learn
        .file_signals()
        .unwrap_or_default()
        .into_iter()
        .filter(|signal| relevant(&signal.path))
        .map(|signal| {
            let prior = file_prior(&signal, now);
            (signal.path, prior)
        })
        .collect();
    let mut combined: BTreeMap<String, f64> = BTreeMap::new();
    for (share, part) in [
        (TASK_SHARE, scaled_map(&tasks)),
        (GLOSSARY_SHARE, scaled_map(&glossary)),
        (HISTORY_SHARE, scaled_map(&history)),
    ] {
        for (path, score) in part {
            *combined.entry(path).or_default() += share * score;
        }
    }
    combined.retain(|_, score| *score > 0.0);
    combined
}

fn file_prior(signal: &FileSignal, now: i64) -> f64 {
    let pivot = if signal.is_pivot() { PIVOT_BONUS } else { 0.0 };
    let raw = f64::from(signal.useful) - NOISE_PENALTY * f64::from(signal.noise) + pivot;
    raw.max(0.0) * decay(now - signal.last_at)
}

fn scaled(values: &[f64]) -> Vec<f64> {
    let top = values.iter().copied().fold(0.0, f64::max);
    if top <= 0.0 {
        return vec![0.0; values.len()];
    }
    values.iter().map(|value| value / top).collect()
}

fn scaled_map(values: &BTreeMap<String, f64>) -> BTreeMap<String, f64> {
    let top = values.values().copied().fold(0.0, f64::max);
    if top <= 0.0 {
        return BTreeMap::new();
    }
    values
        .iter()
        .map(|(path, value)| (path.clone(), value / top))
        .collect()
}

#[allow(
    clippy::cast_precision_loss,
    reason = "a prediction holds a handful of files"
)]
fn mean(values: impl Iterator<Item = f64>) -> f64 {
    let (sum, count) = values.fold((0.0, 0usize), |(sum, count), value| {
        (sum + value, count + 1)
    });
    if count == 0 { 0.0 } else { sum / count as f64 }
}

#[allow(
    clippy::cast_precision_loss,
    reason = "only the ratio of the hash to its range matters"
)]
fn explores(prompt: &str, session: Option<&str>, share: f64) -> bool {
    let mut hasher = DefaultHasher::new();
    prompt.hash(&mut hasher);
    session.hash(&mut hasher);
    (hasher.finish() % 1_000) as f64 / 1_000.0 < share
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use wizard_learn::{Event, EventKind, Learn};
    use wizard_store::{LocatedSymbol, SymbolKind, SymbolRecord};

    use super::{PredictInput, PredictOptions, predict};
    use crate::structure::Structure;
    use crate::vocabulary::Vocabulary;

    fn symbol(path: &str, name: &str, kind: SymbolKind, line: u32) -> LocatedSymbol {
        LocatedSymbol {
            path: path.to_owned(),
            symbol: SymbolRecord {
                kind,
                name: name.to_owned(),
                role: "function".to_owned(),
                line,
                end_line: line + 4,
                signature: format!("fn {name}()"),
                count: 1,
            },
        }
    }

    fn symbols() -> Vec<LocatedSymbol> {
        vec![
            symbol("src/invoice.rs", "compute_total", SymbolKind::Definition, 3),
            symbol("src/tax.rs", "apply_tax", SymbolKind::Definition, 1),
            symbol("src/invoice.rs", "apply_tax", SymbolKind::Reference, 5),
            symbol("src/login.rs", "login", SymbolKind::Definition, 1),
            symbol("src/mailer.rs", "send", SymbolKind::Definition, 1),
            symbol("src/report.rs", "render", SymbolKind::Definition, 1),
            symbol("src/export.rs", "export", SymbolKind::Definition, 1),
            symbol("src/audit.rs", "audit", SymbolKind::Definition, 1),
        ]
    }

    fn quiet() -> PredictOptions {
        PredictOptions {
            exploration: 0.0,
            ..PredictOptions::default()
        }
    }

    #[test]
    fn structure_alone_finds_matching_files_and_their_neighbours() {
        let symbols = symbols();
        let structure = Structure::build(&symbols);
        let vocabulary = Vocabulary::build(&symbols, &structure.paths);
        let input = PredictInput {
            structure: &structure,
            vocabulary: &vocabulary,
            matches: &symbols,
            learn: None,
            prompt: "the invoice total is wrong",
            session: None,
            now: 0,
        };
        let prediction = predict(&input, &quiet());
        assert_eq!(prediction.files[0].path, "src/invoice.rs");
        assert_eq!(prediction.files[0].lines, [(3, 7)]);
        assert!(
            prediction
                .files
                .iter()
                .any(|file| file.path == "src/tax.rs")
        );
        let nothing = PredictInput {
            prompt: "zzz qqq",
            ..input
        };
        assert!(!predict(&nothing, &quiet()).is_confident());
    }

    #[test]
    fn learning_brings_in_files_the_words_do_not_name() {
        let project = tempfile::tempdir().unwrap();
        let mut learn = Learn::open_project(project.path()).unwrap();
        for session in ["a", "b"] {
            learn
                .record(&Event {
                    text: Some("la facture est fausse".to_owned()),
                    ..Event::new(session, 1, EventKind::Prompt)
                })
                .unwrap();
            for path in ["src/invoice.rs", "src/audit.rs"] {
                learn
                    .record(&Event {
                        path: Some(path.to_owned()),
                        ..Event::new(session, 2, EventKind::Edit)
                    })
                    .unwrap();
            }
            learn.consolidate(session).unwrap();
        }
        let symbols = symbols();
        let structure = Structure::build(&symbols);
        let vocabulary = Vocabulary::build(&symbols, &structure.paths);
        let input = PredictInput {
            structure: &structure,
            vocabulary: &vocabulary,
            matches: &symbols,
            learn: Some(&learn),
            prompt: "facture fausse",
            session: Some("now"),
            now: 3,
        };
        let prediction = predict(&input, &quiet());
        let paths: Vec<&str> = prediction
            .files
            .iter()
            .map(|file| file.path.as_str())
            .collect();
        let mut top = paths[..2].to_vec();
        top.sort_unstable();
        assert_eq!(top, ["src/audit.rs", "src/invoice.rs"]);
        assert!(prediction.is_confident(), "{prediction:?}");
    }

    #[test]
    fn exploration_swaps_the_last_file_for_an_unlearned_one() {
        let symbols = symbols();
        let structure = Structure::build(&symbols);
        let vocabulary = Vocabulary::build(&symbols, &structure.paths);
        let input = PredictInput {
            structure: &structure,
            vocabulary: &vocabulary,
            matches: &symbols,
            learn: None,
            prompt: "invoice tax login send render export audit",
            session: None,
            now: 0,
        };
        let always = PredictOptions {
            exploration: 1.0,
            limit: 3,
            ..PredictOptions::default()
        };
        let explored = predict(&input, &always);
        assert!(explored.files[2].explored);
        assert!(!explored.files[..2].iter().any(|file| file.explored));
    }
}
