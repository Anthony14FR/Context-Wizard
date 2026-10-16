//! Domain glossary: the words of the user's tasks mapped to the files that implement them.
//!
//! A word links to a file when at least two sessions used the word, in a prompt or in a search
//! that found nothing, and edited the file, or read it then edited something. Words already
//! in the file's path are left out, as the index finds those without help.

use std::collections::{BTreeMap, BTreeSet};

use wizard_core::text::terms;

use crate::{Learn, LearnError};

/// Number of sessions that must link a word to a file before the glossary keeps the link.
pub const GLOSSARY_MIN_SESSIONS: u32 = 2;

/// A word of the user's tasks and the files it refers to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlossaryEntry {
    /// The word, lowercased.
    pub term: String,
    /// Files linked to the word with the number of sessions linking them, most linked first.
    pub files: Vec<(String, u32)>,
}

impl Learn {
    /// Builds the glossary from every consolidated session, sorted by word.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot be queried.
    pub fn glossary(&self) -> Result<Vec<GlossaryEntry>, LearnError> {
        let mut words: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut statement = self.connection.prepare(
            "SELECT session, text FROM events WHERE kind = 'prompt' AND text IS NOT NULL
             UNION ALL
             SELECT session, pattern FROM gaps",
        )?;
        for row in statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })? {
            let (session, text) = row?;
            words.entry(session).or_default().extend(terms(&text));
        }
        let mut touched: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut statement = self.connection.prepare(
            "SELECT session, path FROM outcomes
             WHERE edited OR session IN (SELECT session FROM outcomes WHERE edited)",
        )?;
        for row in statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })? {
            let (session, path) = row?;
            touched.entry(session).or_default().push(path);
        }
        let mut links: BTreeMap<(String, String), u32> = BTreeMap::new();
        for (session, files) in &touched {
            let Some(terms) = words.get(session) else {
                continue;
            };
            for term in terms {
                for file in files {
                    if !file.to_lowercase().contains(term.as_str()) {
                        *links.entry((term.clone(), file.clone())).or_default() += 1;
                    }
                }
            }
        }
        let mut glossary: BTreeMap<String, Vec<(String, u32)>> = BTreeMap::new();
        for ((term, file), sessions) in links {
            if sessions >= GLOSSARY_MIN_SESSIONS {
                glossary.entry(term).or_default().push((file, sessions));
            }
        }
        Ok(glossary
            .into_iter()
            .map(|(term, mut files)| {
                files.sort_by(|left, right| right.1.cmp(&left.1).then(left.0.cmp(&right.0)));
                GlossaryEntry { term, files }
            })
            .collect())
    }

    /// Files the glossary links to the words of `text`, with the number of linking sessions
    /// summed over those words, most linked first.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot be queried.
    pub fn glossary_files(&self, text: &str) -> Result<Vec<(String, u32)>, LearnError> {
        let wanted: BTreeSet<String> = terms(text).into_iter().collect();
        let mut files: BTreeMap<String, u32> = BTreeMap::new();
        for entry in self.glossary()? {
            if wanted.contains(&entry.term) {
                for (file, sessions) in entry.files {
                    *files.entry(file).or_default() += sessions;
                }
            }
        }
        let mut files: Vec<(String, u32)> = files.into_iter().collect();
        files.sort_by(|left, right| right.1.cmp(&left.1).then(left.0.cmp(&right.0)));
        Ok(files)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::GlossaryEntry;
    use crate::{Event, EventKind, Learn};

    fn session(learn: &mut Learn, id: &str, prompt: &str, empty_search: Option<&str>, edit: &str) {
        learn
            .record(&Event {
                text: Some(prompt.to_owned()),
                ..Event::new(id, 1, EventKind::Prompt)
            })
            .unwrap();
        if let Some(pattern) = empty_search {
            for (pattern, count) in [(pattern, 0), ("fallback", 3)] {
                learn
                    .record(&Event {
                        pattern: Some(pattern.to_owned()),
                        count: Some(count),
                        ..Event::new(id, 2, EventKind::Grep)
                    })
                    .unwrap();
            }
        }
        learn
            .record(&Event {
                path: Some(edit.to_owned()),
                ..Event::new(id, 3, EventKind::Edit)
            })
            .unwrap();
        learn.consolidate(id).unwrap();
    }

    #[test]
    fn links_business_words_to_the_files_sessions_edited() {
        let project = tempfile::tempdir().unwrap();
        let mut learn = Learn::open_project(project.path()).unwrap();
        session(
            &mut learn,
            "a",
            "la facture affiche un mauvais total",
            None,
            "src/invoice.rs",
        );
        session(
            &mut learn,
            "b",
            "arrondi de la facture",
            Some("remise"),
            "src/invoice.rs",
        );
        session(
            &mut learn,
            "c",
            "remise invoice client",
            None,
            "src/invoice.rs",
        );
        session(&mut learn, "d", "facture en double", None, "src/mailer.rs");

        let glossary = learn.glossary().unwrap();
        assert!(glossary.contains(&GlossaryEntry {
            term: "facture".to_owned(),
            files: vec![("src/invoice.rs".to_owned(), 2)],
        }));
        assert!(glossary.contains(&GlossaryEntry {
            term: "remise".to_owned(),
            files: vec![("src/invoice.rs".to_owned(), 2)],
        }));
        assert!(!glossary.iter().any(|entry| entry.term == "invoice"));
        assert_eq!(
            learn.glossary_files("facture remise").unwrap(),
            [("src/invoice.rs".to_owned(), 4)]
        );
    }
}
