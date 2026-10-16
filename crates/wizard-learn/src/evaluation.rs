//! Self-evaluation of the predictions.
//!
//! At each `Stop`, the predictions of the session are graded against the files it edited:
//! precision@5 is the share of predicted files that were edited. Sessions that edited nothing
//! are not graded. When the rolling precision of the last 20 graded sessions drops, the weight
//! of the code structure, α, rises so learning steers less; when precision is high, α slowly
//! falls so learning steers more. α always stays between 0.5 and 0.9.

use std::collections::BTreeSet;

use rusqlite::{OptionalExtension, params};

use crate::{Learn, LearnError};

/// Lowest weight of the code structure in the score.
pub const MIN_ALPHA: f64 = 0.5;
/// Highest weight of the code structure in the score.
pub const MAX_ALPHA: f64 = 0.9;
/// Weight of the code structure before any adjustment.
pub const DEFAULT_ALPHA: f64 = 0.7;
/// Number of graded sessions in the rolling precision.
pub const ROLLING_WINDOW: usize = 20;

const MIN_GRADED: usize = 5;
const RAISE_STEP: f64 = 0.05;
const LOWER_STEP: f64 = 0.02;
const LOW_PRECISION: f64 = 0.4;
const HIGH_PRECISION: f64 = 0.6;
const DROP: f64 = 0.05;
const ALPHA_KEY: &str = "alpha";

/// Grade of one session.
#[derive(Debug, Clone, PartialEq)]
pub struct Evaluation {
    /// Claude Code session identifier.
    pub session: String,
    /// Time of the grading, in milliseconds since the Unix epoch.
    pub at: i64,
    /// Mean precision@5 of the session's predictions, between 0 and 1.
    pub precision: f64,
    /// Number of predictions graded.
    pub predictions: u32,
    /// Weight of the code structure after the grading.
    pub alpha: f64,
}

/// Share of `predicted` files found in `edited`, or `None` when nothing was predicted.
#[must_use]
pub fn precision_at_k(predicted: &[String], edited: &BTreeSet<String>) -> Option<f64> {
    if predicted.is_empty() {
        return None;
    }
    let hits = predicted
        .iter()
        .filter(|file| edited.contains(*file))
        .count();
    Some(ratio(hits, predicted.len()))
}

impl Learn {
    /// Weight of the code structure to use for the next prediction.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot be queried.
    pub fn alpha(&self) -> Result<f64, LearnError> {
        let stored: Option<f64> = self
            .connection
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                [ALPHA_KEY],
                |row| row.get(0),
            )
            .optional()?;
        Ok(stored
            .filter(|alpha| alpha.is_finite())
            .unwrap_or(DEFAULT_ALPHA)
            .clamp(MIN_ALPHA, MAX_ALPHA))
    }

    /// Grades the predictions of `session` against its edits and adjusts α, returning the
    /// grade, or `None` when the session predicted or edited nothing.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot be read or written.
    pub fn evaluate(&mut self, session: &str, now: i64) -> Result<Option<Evaluation>, LearnError> {
        let edited: BTreeSet<String> = self
            .outcomes(session)?
            .into_iter()
            .filter(|outcome| outcome.edited)
            .map(|outcome| outcome.path)
            .collect();
        if edited.is_empty() {
            return Ok(None);
        }
        let grades: Vec<f64> = self
            .predictions(session)?
            .iter()
            .filter_map(|prediction| precision_at_k(&prediction.files, &edited))
            .collect();
        if grades.is_empty() {
            return Ok(None);
        }
        let precision = grades.iter().sum::<f64>() / ratio(grades.len(), 1);
        let before = self.rolling_precision()?;
        self.connection.execute(
            "INSERT INTO evaluations (session, at, precision, predictions, alpha)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (session) DO UPDATE SET
                 at = excluded.at, precision = excluded.precision,
                 predictions = excluded.predictions",
            params![
                session,
                now,
                precision,
                u32::try_from(grades.len()).unwrap_or(u32::MAX),
                self.alpha()?
            ],
        )?;
        let alpha = self.adjust_alpha(before)?;
        self.connection.execute(
            "UPDATE evaluations SET alpha = ?2 WHERE session = ?1",
            params![session, alpha],
        )?;
        Ok(Some(Evaluation {
            session: session.to_owned(),
            at: now,
            precision,
            predictions: u32::try_from(grades.len()).unwrap_or(u32::MAX),
            alpha,
        }))
    }

    /// Mean precision of the last [`ROLLING_WINDOW`] graded sessions, when there are any.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot be queried.
    pub fn rolling_precision(&self) -> Result<Option<f64>, LearnError> {
        let recent = self.evaluations(ROLLING_WINDOW)?;
        if recent.is_empty() {
            return Ok(None);
        }
        Ok(Some(
            recent
                .iter()
                .map(|evaluation| evaluation.precision)
                .sum::<f64>()
                / ratio(recent.len(), 1),
        ))
    }

    /// The last `limit` graded sessions, oldest first.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot be queried.
    pub fn evaluations(&self, limit: usize) -> Result<Vec<Evaluation>, LearnError> {
        let mut statement = self.connection.prepare(
            "SELECT session, at, precision, predictions, alpha FROM evaluations
             ORDER BY at DESC, session DESC LIMIT ?1",
        )?;
        let mut evaluations: Vec<Evaluation> = statement
            .query_map([i64::try_from(limit).unwrap_or(i64::MAX)], |row| {
                Ok(Evaluation {
                    session: row.get(0)?,
                    at: row.get(1)?,
                    precision: row.get(2)?,
                    predictions: row.get(3)?,
                    alpha: row.get(4)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        evaluations.reverse();
        Ok(evaluations)
    }

    fn adjust_alpha(&mut self, before: Option<f64>) -> Result<f64, LearnError> {
        let alpha = self.alpha()?;
        let graded = self.evaluations(ROLLING_WINDOW)?.len();
        let Some(now) = self.rolling_precision()? else {
            return Ok(alpha);
        };
        let dropped = before.is_some_and(|before| now < before - DROP);
        let next = if graded < MIN_GRADED {
            alpha
        } else if dropped || now < LOW_PRECISION {
            alpha + RAISE_STEP
        } else if now >= HIGH_PRECISION {
            alpha - LOWER_STEP
        } else {
            alpha
        }
        .clamp(MIN_ALPHA, MAX_ALPHA);
        self.connection.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            params![ALPHA_KEY, next],
        )?;
        Ok(next)
    }
}

#[allow(
    clippy::cast_precision_loss,
    reason = "counts of files and sessions stay far below 2^52"
)]
fn ratio(numerator: usize, denominator: usize) -> f64 {
    numerator as f64 / denominator as f64
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::collections::BTreeSet;

    use super::{DEFAULT_ALPHA, MAX_ALPHA, precision_at_k};
    use crate::{Event, EventKind, Learn, StoredPrediction};

    fn graded_session(learn: &mut Learn, id: &str, at: i64, predicted: &[&str], edited: &str) {
        learn
            .record_prediction(&StoredPrediction {
                session: id.to_owned(),
                at,
                files: predicted.iter().map(|file| (*file).to_owned()).collect(),
                confidence: 0.5,
                alpha: 0.7,
                shown: true,
            })
            .unwrap();
        learn
            .record(&Event {
                path: Some(edited.to_owned()),
                ..Event::new(id, at + 1, EventKind::Edit)
            })
            .unwrap();
        learn.consolidate(id).unwrap();
    }

    #[test]
    fn precision_counts_predicted_files_that_were_edited() {
        let edited = BTreeSet::from(["a.rs".to_owned(), "c.rs".to_owned()]);
        let predicted = ["a.rs", "b.rs", "c.rs", "d.rs"].map(str::to_owned);
        assert_eq!(precision_at_k(&predicted, &edited), Some(0.5));
        assert_eq!(precision_at_k(&[], &edited), None);
    }

    #[test]
    fn grades_sessions_and_raises_alpha_when_precision_is_low() {
        let project = tempfile::tempdir().unwrap();
        let mut learn = Learn::open_project(project.path()).unwrap();
        assert!((learn.alpha().unwrap() - DEFAULT_ALPHA).abs() < 1e-12);
        learn
            .record(&Event::new("idle", 1, EventKind::Stop))
            .unwrap();
        assert_eq!(learn.evaluate("idle", 2).unwrap(), None);

        graded_session(&mut learn, "s0", 10, &["a.rs", "b.rs"], "a.rs");
        let first = learn.evaluate("s0", 12).unwrap().unwrap();
        assert!((first.precision - 0.5).abs() < 1e-12);
        assert!((first.alpha - DEFAULT_ALPHA).abs() < 1e-12);
        for index in 1..10 {
            let id = format!("s{index}");
            graded_session(&mut learn, &id, 10 * index, &["x.rs", "y.rs"], "a.rs");
            learn.evaluate(&id, 10 * index + 2).unwrap();
        }
        let rolling = learn.rolling_precision().unwrap().unwrap();
        assert!(rolling < 0.1, "{rolling}");
        assert!(learn.alpha().unwrap() > DEFAULT_ALPHA);
        assert!(learn.alpha().unwrap() <= MAX_ALPHA);
        assert_eq!(learn.evaluations(20).unwrap().len(), 10);
        assert_eq!(learn.evaluations(20).unwrap()[0].session, "s0");
    }

    #[test]
    fn lowers_alpha_slowly_when_predictions_are_good() {
        let project = tempfile::tempdir().unwrap();
        let mut learn = Learn::open_project(project.path()).unwrap();
        for index in 0..8 {
            let id = format!("s{index}");
            graded_session(&mut learn, &id, 10 * index, &["a.rs"], "a.rs");
            learn.evaluate(&id, 10 * index + 2).unwrap();
        }
        let alpha = learn.alpha().unwrap();
        assert!(
            alpha < DEFAULT_ALPHA && alpha > DEFAULT_ALPHA - 0.1,
            "{alpha}"
        );
    }
}
