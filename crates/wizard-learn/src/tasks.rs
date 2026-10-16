//! Matching a new task against past sessions.
//!
//! Each consolidated session is a document made of its prompts. BM25 ranks those documents
//! against the new prompt, and the files the best sessions edited, and to a lesser extent read,
//! become the files predicted for the new task.

use std::collections::{BTreeMap, HashMap};

use wizard_core::text::terms;

use crate::{Learn, LearnError, decay};

const K1: f64 = 1.2;
const B: f64 = 0.75;
const READ_WEIGHT: f64 = 0.25;

/// Number of most similar sessions whose files are predicted.
pub const SIMILAR_SESSIONS: usize = 5;

/// A past session similar to a task.
#[derive(Debug, Clone, PartialEq)]
pub struct SimilarSession {
    /// Claude Code session identifier.
    pub session: String,
    /// BM25 score of the session's prompts against the task.
    pub score: f64,
}

/// A file predicted for a task from similar sessions.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskFile {
    /// File, relative to the project root.
    pub path: String,
    /// Sum over similar sessions of their score, times how the file was used, faded by age.
    pub score: f64,
}

/// Ranks `documents` against `query` with BM25, best first, leaving out documents that share no
/// term with the query.
#[must_use]
#[allow(
    clippy::cast_precision_loss,
    reason = "document and term counts stay far below 2^52"
)]
pub fn bm25<'a>(query: &[String], documents: &'a [(String, Vec<String>)]) -> Vec<(&'a str, f64)> {
    if documents.is_empty() || query.is_empty() {
        return Vec::new();
    }
    let count = documents.len() as f64;
    let average = documents
        .iter()
        .map(|(_, terms)| terms.len() as f64)
        .sum::<f64>()
        / count;
    let mut frequency: HashMap<&str, f64> = HashMap::new();
    for (_, terms) in documents {
        let mut seen: Vec<&str> = terms.iter().map(String::as_str).collect();
        seen.sort_unstable();
        seen.dedup();
        for term in seen {
            *frequency.entry(term).or_default() += 1.0;
        }
    }
    let mut query: Vec<&str> = query.iter().map(String::as_str).collect();
    query.sort_unstable();
    query.dedup();
    let mut scored: Vec<(&str, f64)> = documents
        .iter()
        .filter_map(|(id, terms)| {
            let length = terms.len() as f64;
            let score: f64 = query
                .iter()
                .filter_map(|term| {
                    let occurrences = terms.iter().filter(|candidate| candidate == term).count();
                    if occurrences == 0 {
                        return None;
                    }
                    let occurrences = occurrences as f64;
                    let documents_with = frequency.get(term).copied().unwrap_or(0.0);
                    let idf = ((count - documents_with + 0.5) / (documents_with + 0.5) + 1.0).ln();
                    let norm = K1 * (1.0 - B + B * length / average.max(1.0));
                    Some(idf * occurrences * (K1 + 1.0) / (occurrences + norm))
                })
                .sum();
            (score > 0.0).then_some((id.as_str(), score))
        })
        .collect();
    scored.sort_by(|left, right| right.1.total_cmp(&left.1).then(left.0.cmp(right.0)));
    scored
}

impl Learn {
    /// Past consolidated sessions most similar to `prompt`, best first, excluding `current`.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot be queried.
    pub fn similar_sessions(
        &self,
        prompt: &str,
        current: Option<&str>,
    ) -> Result<Vec<SimilarSession>, LearnError> {
        let mut statement = self.connection.prepare(
            "SELECT e.session, e.text FROM events e
             WHERE e.kind = 'prompt' AND e.text IS NOT NULL
               AND EXISTS (SELECT 1 FROM outcomes o WHERE o.session = e.session)
             ORDER BY e.id",
        )?;
        let mut prompts: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for row in statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })? {
            let (session, text) = row?;
            if Some(session.as_str()) != current {
                prompts.entry(session).or_default().extend(terms(&text));
            }
        }
        let documents: Vec<(String, Vec<String>)> = prompts.into_iter().collect();
        Ok(bm25(&terms(prompt), &documents)
            .into_iter()
            .take(SIMILAR_SESSIONS)
            .map(|(session, score)| SimilarSession {
                session: session.to_owned(),
                score,
            })
            .collect())
    }

    /// Files predicted for `prompt` from the most similar past sessions, best first.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot be queried.
    pub fn task_files(
        &self,
        prompt: &str,
        current: Option<&str>,
        now: i64,
    ) -> Result<Vec<TaskFile>, LearnError> {
        let mut scores: BTreeMap<String, f64> = BTreeMap::new();
        for similar in self.similar_sessions(prompt, current)? {
            for outcome in self.outcomes(&similar.session)? {
                let usage = if outcome.edited { 1.0 } else { READ_WEIGHT };
                *scores.entry(outcome.path).or_default() +=
                    similar.score * usage * decay(now - outcome.at);
            }
        }
        let mut files: Vec<TaskFile> = scores
            .into_iter()
            .map(|(path, score)| TaskFile { path, score })
            .collect();
        files.sort_by(|left, right| {
            right
                .score
                .total_cmp(&left.score)
                .then(left.path.cmp(&right.path))
        });
        Ok(files)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use wizard_core::text::terms;

    use super::bm25;
    use crate::{Event, EventKind, Learn};

    fn session(learn: &mut Learn, id: &str, at: i64, prompt: &str, read: &str, edit: &str) {
        learn
            .record(&Event {
                text: Some(prompt.to_owned()),
                ..Event::new(id, at, EventKind::Prompt)
            })
            .unwrap();
        for (kind, path) in [(EventKind::Read, read), (EventKind::Edit, edit)] {
            learn
                .record(&Event {
                    path: Some(path.to_owned()),
                    ..Event::new(id, at + 1, kind)
                })
                .unwrap();
        }
        learn
            .record(&Event::new(id, at + 2, EventKind::Stop))
            .unwrap();
        learn.consolidate(id).unwrap();
    }

    #[test]
    fn ranks_documents_sharing_rare_terms_first() {
        let documents = vec![
            ("a".to_owned(), terms("invoice totals are wrong in billing")),
            ("b".to_owned(), terms("login page crashes on submit")),
            ("c".to_owned(), terms("billing page layout")),
        ];
        let ranked = bm25(&terms("wrong invoice totals"), &documents);
        assert_eq!(ranked[0].0, "a");
        assert_eq!(ranked.len(), 1);
        assert_eq!(bm25(&terms("unrelated words"), &documents), Vec::new());
    }

    #[test]
    fn predicts_files_edited_in_similar_sessions() {
        let project = tempfile::tempdir().unwrap();
        let mut learn = Learn::open_project(project.path()).unwrap();
        session(
            &mut learn,
            "a",
            10,
            "invoice totals are wrong",
            "src/tax.rs",
            "src/invoice.rs",
        );
        session(
            &mut learn,
            "b",
            20,
            "login page crashes",
            "src/page.rs",
            "src/login.rs",
        );
        learn
            .record(&Event {
                text: Some("invoice rounding".to_owned()),
                ..Event::new("now", 30, EventKind::Prompt)
            })
            .unwrap();

        let similar = learn
            .similar_sessions("invoice rounding", Some("now"))
            .unwrap();
        assert_eq!(similar.len(), 1);
        assert_eq!(similar[0].session, "a");
        let files = learn
            .task_files("invoice rounding", Some("now"), 30)
            .unwrap();
        let paths: Vec<&str> = files.iter().map(|file| file.path.as_str()).collect();
        assert_eq!(paths, ["src/invoice.rs", "src/tax.rs"]);
        assert!(files[0].score > files[1].score * 3.0);
    }
}
