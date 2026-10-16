//! Predictions made for each prompt, kept so each session can grade them.

use rusqlite::{OptionalExtension, Row, TransactionBehavior, params};

use crate::{Learn, LearnError};

/// A prediction recorded for a prompt.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredPrediction {
    /// Claude Code session identifier.
    pub session: String,
    /// Time of the prediction, in milliseconds since the Unix epoch.
    pub at: i64,
    /// Predicted files, best first.
    pub files: Vec<String>,
    /// Confidence of the prediction, between 0 and 1.
    pub confidence: f64,
    /// Weight of the structural part used for it.
    pub alpha: f64,
    /// Whether it was confident enough to be shown to Claude.
    pub shown: bool,
}

impl Learn {
    /// Records `prediction`, creating its session on first sight.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot be written.
    pub fn record_prediction(&mut self, prediction: &StoredPrediction) -> Result<(), LearnError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute(
            "INSERT OR IGNORE INTO sessions (id, started) VALUES (?1, ?2)",
            params![prediction.session, prediction.at],
        )?;
        transaction.execute(
            "INSERT INTO predictions (session, at, files, confidence, alpha, shown)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                prediction.session,
                prediction.at,
                prediction.files.join("\n"),
                prediction.confidence,
                prediction.alpha,
                prediction.shown,
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Returns the predictions of `session`, oldest first.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot be queried.
    pub fn predictions(&self, session: &str) -> Result<Vec<StoredPrediction>, LearnError> {
        let mut statement = self.connection.prepare(
            "SELECT session, at, files, confidence, alpha, shown FROM predictions
             WHERE session = ?1 ORDER BY id",
        )?;
        let predictions = statement
            .query_map([session], prediction_of_row)?
            .collect::<Result<_, _>>()?;
        Ok(predictions)
    }

    /// Returns the latest prediction of `session`, when it has one.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot be queried.
    pub fn latest_prediction(&self, session: &str) -> Result<Option<StoredPrediction>, LearnError> {
        Ok(self
            .connection
            .query_row(
                "SELECT session, at, files, confidence, alpha, shown FROM predictions
                 WHERE session = ?1 ORDER BY id DESC LIMIT 1",
                [session],
                prediction_of_row,
            )
            .optional()?)
    }
}

fn prediction_of_row(row: &Row<'_>) -> rusqlite::Result<StoredPrediction> {
    let files: String = row.get(2)?;
    Ok(StoredPrediction {
        session: row.get(0)?,
        at: row.get(1)?,
        files: files
            .split('\n')
            .filter(|file| !file.is_empty())
            .map(str::to_owned)
            .collect(),
        confidence: row.get(3)?,
        alpha: row.get(4)?,
        shown: row.get(5)?,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::StoredPrediction;
    use crate::Learn;

    #[test]
    fn keeps_predictions_by_session() {
        let project = tempfile::tempdir().unwrap();
        let mut learn = Learn::open_project(project.path()).unwrap();
        let first = StoredPrediction {
            session: "s".to_owned(),
            at: 1,
            files: vec!["a.rs".to_owned(), "b.rs".to_owned()],
            confidence: 0.5,
            alpha: 0.7,
            shown: true,
        };
        let second = StoredPrediction {
            at: 2,
            files: Vec::new(),
            shown: false,
            ..first.clone()
        };
        learn.record_prediction(&first).unwrap();
        learn.record_prediction(&second).unwrap();
        assert_eq!(learn.predictions("s").unwrap(), [first, second.clone()]);
        assert_eq!(learn.latest_prediction("s").unwrap(), Some(second));
        assert_eq!(learn.latest_prediction("other").unwrap(), None);
    }
}
