//! Consolidation of a session's events into learning signals.
//!
//! Each `Stop` consolidates its session again, so the outcome is the same however many answers
//! the session holds. The five signals are derived from what is stored here:
//!
//! - read then edited in a session: useful;
//! - read and never edited in a session: noise;
//! - empty search followed by a different search: a gap in the map;
//! - read in several sessions: pivot file;
//! - files touched in the same session: co-access.

use std::collections::BTreeMap;

use rusqlite::{TransactionBehavior, params};

use crate::{Event, EventKind, Learn, LearnError};

/// Number of sessions reading a file from which it counts as a pivot file.
pub const PIVOT_SESSIONS: u32 = 3;

/// Sessions touching more files than this are left out of co-access, as they link files at random.
pub const CO_ACCESS_MAX_FILES: i64 = 40;

/// What one session did with one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// File, relative to the project root.
    pub path: String,
    /// Number of reads of the file.
    pub reads: u32,
    /// Whether the file was modified.
    pub edited: bool,
    /// Time of the last event on the file, in milliseconds since the Unix epoch.
    pub at: i64,
}

/// A search that found nothing and was followed by a different search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gap {
    /// The pattern that found nothing.
    pub pattern: String,
    /// The next pattern searched.
    pub followed_by: String,
    /// Time of the empty search, in milliseconds since the Unix epoch.
    pub at: i64,
}

/// Signals learned about one file across sessions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSignal {
    /// File, relative to the project root.
    pub path: String,
    /// Sessions that read the file, then edited it.
    pub useful: u32,
    /// Sessions that read the file and never edited it.
    pub noise: u32,
    /// Sessions that edited the file.
    pub edited: u32,
    /// Sessions that read the file.
    pub read_sessions: u32,
    /// Time of the latest event on the file, in milliseconds since the Unix epoch.
    pub last_at: i64,
}

impl FileSignal {
    /// Returns `true` when enough sessions read the file to make it a pivot.
    #[must_use]
    pub fn is_pivot(&self) -> bool {
        self.read_sessions >= PIVOT_SESSIONS
    }
}

/// Two files touched in the same sessions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoAccess {
    /// First file, the smaller path.
    pub first: String,
    /// Second file.
    pub second: String,
    /// Number of sessions that touched both.
    pub sessions: u32,
    /// Time of the latest of those sessions, in milliseconds since the Unix epoch.
    pub last_at: i64,
}

/// Derives the file outcomes and the map gaps of one session from its events.
#[must_use]
pub fn consolidate_events(events: &[Event]) -> (Vec<Outcome>, Vec<Gap>) {
    let mut outcomes: BTreeMap<&str, Outcome> = BTreeMap::new();
    let mut gaps: Vec<Gap> = Vec::new();
    let mut empty: Option<(&str, i64)> = None;
    for event in events {
        match event.kind {
            EventKind::Read | EventKind::Edit => {
                let Some(path) = event.path.as_deref() else {
                    continue;
                };
                let outcome = outcomes.entry(path).or_insert_with(|| Outcome {
                    path: path.to_owned(),
                    reads: 0,
                    edited: false,
                    at: event.at,
                });
                if event.kind == EventKind::Read {
                    outcome.reads += 1;
                } else {
                    outcome.edited = true;
                }
                outcome.at = outcome.at.max(event.at);
            }
            EventKind::Grep | EventKind::Glob => {
                let Some(pattern) = event.pattern.as_deref() else {
                    continue;
                };
                if let Some((previous, at)) = empty
                    && previous != pattern
                    && !gaps.iter().any(|gap| gap.pattern == previous)
                {
                    gaps.push(Gap {
                        pattern: previous.to_owned(),
                        followed_by: pattern.to_owned(),
                        at,
                    });
                }
                empty = (event.count == Some(0)).then_some((pattern, event.at));
            }
            EventKind::Prompt | EventKind::Stop => empty = None,
        }
    }
    (outcomes.into_values().collect(), gaps)
}

impl Learn {
    /// Recomputes the outcomes and gaps of `session` from its events.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot be read or written.
    pub fn consolidate(&mut self, session: &str) -> Result<(), LearnError> {
        let events = self.events(session)?;
        let (outcomes, gaps) = consolidate_events(&events);
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute("DELETE FROM outcomes WHERE session = ?1", [session])?;
        transaction.execute("DELETE FROM gaps WHERE session = ?1", [session])?;
        for outcome in &outcomes {
            transaction.execute(
                "INSERT INTO outcomes (session, path, reads, edited, at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    session,
                    outcome.path,
                    outcome.reads,
                    outcome.edited,
                    outcome.at
                ],
            )?;
        }
        for gap in &gaps {
            transaction.execute(
                "INSERT INTO gaps (session, pattern, followed_by, at) VALUES (?1, ?2, ?3, ?4)",
                params![session, gap.pattern, gap.followed_by, gap.at],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    /// Returns the outcomes of `session`, sorted by path.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot be queried.
    pub fn outcomes(&self, session: &str) -> Result<Vec<Outcome>, LearnError> {
        let mut statement = self.connection.prepare(
            "SELECT path, reads, edited, at FROM outcomes WHERE session = ?1 ORDER BY path",
        )?;
        let outcomes = statement
            .query_map([session], |row| {
                Ok(Outcome {
                    path: row.get(0)?,
                    reads: row.get(1)?,
                    edited: row.get(2)?,
                    at: row.get(3)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        Ok(outcomes)
    }

    /// Returns the signals of every file seen in a consolidated session, sorted by path.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot be queried.
    pub fn file_signals(&self) -> Result<Vec<FileSignal>, LearnError> {
        let mut statement = self.connection.prepare(
            "SELECT path,
                    SUM(reads > 0 AND edited),
                    SUM(reads > 0 AND NOT edited),
                    SUM(edited),
                    SUM(reads > 0),
                    MAX(at)
             FROM outcomes GROUP BY path ORDER BY path",
        )?;
        let signals = statement
            .query_map([], |row| {
                Ok(FileSignal {
                    path: row.get(0)?,
                    useful: row.get(1)?,
                    noise: row.get(2)?,
                    edited: row.get(3)?,
                    read_sessions: row.get(4)?,
                    last_at: row.get(5)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        Ok(signals)
    }

    /// Returns the pairs of files touched in the same sessions, most shared first.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot be queried.
    pub fn co_access(&self) -> Result<Vec<CoAccess>, LearnError> {
        let mut statement = self.connection.prepare(
            "WITH small AS (
                 SELECT session FROM outcomes GROUP BY session HAVING COUNT(*) <= ?1
             )
             SELECT a.path, b.path, COUNT(*), MAX(MAX(a.at, b.at))
             FROM outcomes a
             JOIN outcomes b ON a.session = b.session AND a.path < b.path
             WHERE a.session IN small
             GROUP BY a.path, b.path
             ORDER BY COUNT(*) DESC, a.path, b.path",
        )?;
        let pairs = statement
            .query_map([CO_ACCESS_MAX_FILES], |row| {
                Ok(CoAccess {
                    first: row.get(0)?,
                    second: row.get(1)?,
                    sessions: row.get(2)?,
                    last_at: row.get(3)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        Ok(pairs)
    }

    /// Returns every recorded gap, oldest first.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot be queried.
    pub fn gaps(&self) -> Result<Vec<Gap>, LearnError> {
        let mut statement = self
            .connection
            .prepare("SELECT pattern, followed_by, at FROM gaps ORDER BY at, pattern")?;
        let gaps = statement
            .query_map([], |row| {
                Ok(Gap {
                    pattern: row.get(0)?,
                    followed_by: row.get(1)?,
                    at: row.get(2)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        Ok(gaps)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::{CoAccess, Gap, consolidate_events};
    use crate::{Event, EventKind, Learn};

    fn on(session: &str, at: i64, kind: EventKind, path: &str) -> Event {
        Event {
            path: Some(path.to_owned()),
            ..Event::new(session, at, kind)
        }
    }

    fn search(session: &str, at: i64, pattern: &str, count: i64) -> Event {
        Event {
            pattern: Some(pattern.to_owned()),
            count: Some(count),
            ..Event::new(session, at, EventKind::Grep)
        }
    }

    #[test]
    fn finds_useful_and_noise_files_and_gaps() {
        let events = [
            Event::new("s", 1, EventKind::Prompt),
            search("s", 2, "invoiceTotal", 0),
            search("s", 3, "invoice", 4),
            on("s", 4, EventKind::Read, "src/billing.rs"),
            on("s", 5, EventKind::Read, "src/readme.md"),
            on("s", 6, EventKind::Edit, "src/billing.rs"),
            on("s", 7, EventKind::Read, "src/billing.rs"),
            search("s", 8, "nothing", 0),
            Event::new("s", 9, EventKind::Stop),
            search("s", 10, "else", 1),
        ];
        let (outcomes, gaps) = consolidate_events(&events);
        let summary: Vec<(&str, u32, bool, i64)> = outcomes
            .iter()
            .map(|outcome| {
                (
                    outcome.path.as_str(),
                    outcome.reads,
                    outcome.edited,
                    outcome.at,
                )
            })
            .collect();
        assert_eq!(
            summary,
            [
                ("src/billing.rs", 2, true, 7),
                ("src/readme.md", 1, false, 5)
            ]
        );
        assert_eq!(
            gaps,
            [Gap {
                pattern: "invoiceTotal".to_owned(),
                followed_by: "invoice".to_owned(),
                at: 2
            }]
        );
    }

    #[test]
    fn aggregates_signals_across_sessions() {
        let project = tempfile::tempdir().unwrap();
        let mut learn = Learn::open_project(project.path()).unwrap();
        for (index, session) in ["a", "b", "c"].into_iter().enumerate() {
            let at = i64::try_from(index).unwrap() * 10;
            learn
                .record(&on(session, at, EventKind::Read, "core.rs"))
                .unwrap();
            learn
                .record(&on(session, at + 1, EventKind::Read, "other.rs"))
                .unwrap();
            if session == "a" {
                learn
                    .record(&on(session, at + 2, EventKind::Edit, "core.rs"))
                    .unwrap();
            }
            learn
                .record(&Event::new(session, at + 3, EventKind::Stop))
                .unwrap();
            learn.consolidate(session).unwrap();
            learn.consolidate(session).unwrap();
        }
        let signals = learn.file_signals().unwrap();
        assert_eq!(signals.len(), 2);
        let core = &signals[0];
        assert_eq!(core.path, "core.rs");
        assert_eq!((core.useful, core.noise, core.edited), (1, 2, 1));
        assert!(core.is_pivot());
        assert_eq!(core.last_at, 20);
        assert_eq!(
            learn.co_access().unwrap(),
            [CoAccess {
                first: "core.rs".to_owned(),
                second: "other.rs".to_owned(),
                sessions: 3,
                last_at: 21
            }]
        );
        assert_eq!(learn.outcomes("a").unwrap().len(), 2);
    }
}
