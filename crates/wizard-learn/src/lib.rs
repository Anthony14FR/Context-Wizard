//! Learning store of Context Wizard.
//!
//! Hooks record what happens in each Claude Code session of an enabled project: the prompts,
//! the files read, the searches and their result counts, the files edited, and the end of each
//! answer. Events are linked by session and kept in `.wizard/learn.db`. Each `Stop`
//! consolidates its session into per-file outcomes and map gaps, from which the learning
//! signals are read.
//!
//! Unlike the index, this store cannot be rebuilt from the project, so a schema change migrates
//! it instead of dropping it. A database written by a newer build is left untouched, and a
//! corrupt one is moved aside before a fresh store is created.

use std::error::Error;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, ErrorCode, OptionalExtension, Row, TransactionBehavior, params};
use wizard_store::{StoreError, data_directory};

mod signals;

pub use signals::{
    CO_ACCESS_MAX_FILES, CoAccess, FileSignal, Gap, Outcome, PIVOT_SESSIONS, consolidate_events,
};

/// File name of the learning store inside the `.wizard` directory.
pub const LEARN_FILE: &str = "learn.db";

/// Version of the learning store schema written by this build.
pub const SCHEMA_VERSION: i64 = 2;

const OPEN_ATTEMPTS: u32 = 50;
const OPEN_BACKOFF: Duration = Duration::from_millis(20);

const MIGRATIONS: [&str; 2] = [
    "
    CREATE TABLE sessions (
        id TEXT PRIMARY KEY NOT NULL,
        started INTEGER NOT NULL,
        ended INTEGER
    ) STRICT;
    CREATE TABLE events (
        id INTEGER PRIMARY KEY,
        session TEXT NOT NULL REFERENCES sessions (id) ON DELETE CASCADE,
        at INTEGER NOT NULL,
        kind TEXT NOT NULL,
        path TEXT,
        pattern TEXT,
        start_line INTEGER,
        end_line INTEGER,
        count INTEGER,
        text TEXT
    ) STRICT;
    CREATE INDEX events_by_session ON events (session, id);
    CREATE INDEX events_by_path ON events (path);
",
    "
    CREATE TABLE outcomes (
        session TEXT NOT NULL REFERENCES sessions (id) ON DELETE CASCADE,
        path TEXT NOT NULL,
        reads INTEGER NOT NULL,
        edited INTEGER NOT NULL,
        at INTEGER NOT NULL,
        PRIMARY KEY (session, path)
    ) STRICT;
    CREATE INDEX outcomes_by_path ON outcomes (path);
    CREATE TABLE gaps (
        session TEXT NOT NULL REFERENCES sessions (id) ON DELETE CASCADE,
        pattern TEXT NOT NULL,
        followed_by TEXT NOT NULL,
        at INTEGER NOT NULL,
        PRIMARY KEY (session, pattern)
    ) STRICT;
",
];

/// Failure to open or update the learning store.
#[derive(Debug)]
#[non_exhaustive]
pub enum LearnError {
    /// The data directory could not be prepared.
    Store(StoreError),
    /// SQLite reported an error.
    Sqlite(rusqlite::Error),
    /// The store was written by a newer build and is left untouched.
    NewerSchema(i64),
}

impl fmt::Display for LearnError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(source) => source.fmt(formatter),
            Self::Sqlite(source) => write!(formatter, "learning database error: {source}"),
            Self::NewerSchema(version) => write!(
                formatter,
                "learning database schema {version} is newer than this build supports ({SCHEMA_VERSION})"
            ),
        }
    }
}

impl Error for LearnError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Store(source) => Some(source),
            Self::Sqlite(source) => Some(source),
            Self::NewerSchema(_) => None,
        }
    }
}

impl From<rusqlite::Error> for LearnError {
    fn from(source: rusqlite::Error) -> Self {
        Self::Sqlite(source)
    }
}

impl From<StoreError> for LearnError {
    fn from(source: StoreError) -> Self {
        Self::Store(source)
    }
}

/// What a recorded event describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EventKind {
    /// The user submitted a prompt.
    Prompt,
    /// Claude read a file.
    Read,
    /// Claude searched file contents.
    Grep,
    /// Claude listed files by pattern.
    Glob,
    /// Claude modified or created a file.
    Edit,
    /// Claude finished answering.
    Stop,
}

impl EventKind {
    /// Name stored in the database.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Prompt => "prompt",
            Self::Read => "read",
            Self::Grep => "grep",
            Self::Glob => "glob",
            Self::Edit => "edit",
            Self::Stop => "stop",
        }
    }

    fn parse(name: &str) -> Option<Self> {
        [
            Self::Prompt,
            Self::Read,
            Self::Grep,
            Self::Glob,
            Self::Edit,
            Self::Stop,
        ]
        .into_iter()
        .find(|kind| kind.as_str() == name)
    }
}

/// One event of a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    /// Claude Code session identifier.
    pub session: String,
    /// Time of the event, in milliseconds since the Unix epoch.
    pub at: i64,
    /// What the event describes.
    pub kind: EventKind,
    /// File concerned, relative to the project root with `/` separators.
    pub path: Option<String>,
    /// Search pattern, for searches.
    pub pattern: Option<String>,
    /// First line read, counted from 1.
    pub start_line: Option<i64>,
    /// Last line read, counted from 1.
    pub end_line: Option<i64>,
    /// Number of results, for searches.
    pub count: Option<i64>,
    /// Prompt text, for prompts.
    pub text: Option<String>,
}

impl Event {
    /// Creates an event of `kind` in `session` at `at`, with no other detail.
    #[must_use]
    pub fn new(session: impl Into<String>, at: i64, kind: EventKind) -> Self {
        Self {
            session: session.into(),
            at,
            kind,
            path: None,
            pattern: None,
            start_line: None,
            end_line: None,
            count: None,
            text: None,
        }
    }
}

/// A recorded session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    /// Claude Code session identifier.
    pub id: String,
    /// Time of its first event, in milliseconds since the Unix epoch.
    pub started: i64,
    /// Time of its latest `Stop`, when Claude has answered at least once.
    pub ended: Option<i64>,
}

/// The learning store of one project.
#[derive(Debug)]
pub struct Learn {
    connection: Connection,
}

impl Learn {
    /// Opens the learning store of `project`, creating it when missing.
    ///
    /// Callers check the project's opt-in decision first: this creates the `.wizard` directory.
    ///
    /// # Errors
    ///
    /// Returns an error when the store cannot be opened, or was written by a newer build.
    pub fn open_project(project: &Path) -> Result<Self, LearnError> {
        Self::open(&data_directory(project)?.join(LEARN_FILE))
    }

    /// Opens the learning store at `path`, creating or migrating it as needed.
    ///
    /// # Errors
    ///
    /// Returns an error when the store cannot be opened, or was written by a newer build.
    pub fn open(path: &Path) -> Result<Self, LearnError> {
        match connect(path) {
            Err(LearnError::Sqlite(error)) if is_corruption(&error) => {
                set_aside(path);
                connect(path)
            }
            result => result,
        }
    }

    /// Records `event`, creating its session on first sight and closing it on `Stop`.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot be written.
    pub fn record(&mut self, event: &Event) -> Result<(), LearnError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute(
            "INSERT OR IGNORE INTO sessions (id, started) VALUES (?1, ?2)",
            params![event.session, event.at],
        )?;
        transaction.execute(
            "INSERT INTO events (session, at, kind, path, pattern, start_line, end_line, count, text)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                event.session,
                event.at,
                event.kind.as_str(),
                event.path,
                event.pattern,
                event.start_line,
                event.end_line,
                event.count,
                event.text,
            ],
        )?;
        if event.kind == EventKind::Stop {
            transaction.execute(
                "UPDATE sessions SET ended = ?2 WHERE id = ?1",
                params![event.session, event.at],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    /// Returns the session `id`, when it was recorded.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot be queried.
    pub fn session(&self, id: &str) -> Result<Option<Session>, LearnError> {
        Ok(self
            .connection
            .query_row(
                "SELECT id, started, ended FROM sessions WHERE id = ?1",
                [id],
                session_of_row,
            )
            .optional()?)
    }

    /// Returns every recorded session, oldest first.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot be queried.
    pub fn sessions(&self) -> Result<Vec<Session>, LearnError> {
        let mut statement = self
            .connection
            .prepare("SELECT id, started, ended FROM sessions ORDER BY started, id")?;
        let sessions = statement
            .query_map([], session_of_row)?
            .collect::<Result<_, _>>()?;
        Ok(sessions)
    }

    /// Returns the events of `session` in the order they were recorded.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot be queried.
    pub fn events(&self, session: &str) -> Result<Vec<Event>, LearnError> {
        let mut statement = self.connection.prepare(
            "SELECT session, at, kind, path, pattern, start_line, end_line, count, text
             FROM events WHERE session = ?1 ORDER BY id",
        )?;
        let events = statement
            .query_map([session], event_of_row)?
            .filter_map(Result::transpose)
            .collect::<Result<_, _>>()?;
        Ok(events)
    }
}

/// Current time in milliseconds since the Unix epoch.
#[must_use]
pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|elapsed| i64::try_from(elapsed.as_millis()).ok())
        .unwrap_or(0)
}

fn session_of_row(row: &Row<'_>) -> rusqlite::Result<Session> {
    Ok(Session {
        id: row.get(0)?,
        started: row.get(1)?,
        ended: row.get(2)?,
    })
}

fn event_of_row(row: &Row<'_>) -> rusqlite::Result<Option<Event>> {
    let kind: String = row.get(2)?;
    let Some(kind) = EventKind::parse(&kind) else {
        return Ok(None);
    };
    Ok(Some(Event {
        session: row.get(0)?,
        at: row.get(1)?,
        kind,
        path: row.get(3)?,
        pattern: row.get(4)?,
        start_line: row.get(5)?,
        end_line: row.get(6)?,
        count: row.get(7)?,
        text: row.get(8)?,
    }))
}

fn connect(path: &Path) -> Result<Learn, LearnError> {
    let mut attempt = 1;
    loop {
        match try_connect(path) {
            Err(LearnError::Sqlite(error))
                if error.sqlite_error_code() == Some(ErrorCode::DatabaseBusy)
                    && attempt < OPEN_ATTEMPTS =>
            {
                attempt += 1;
                std::thread::sleep(OPEN_BACKOFF);
            }
            result => return result,
        }
    }
}

fn try_connect(path: &Path) -> Result<Learn, LearnError> {
    let mut connection = Connection::open(path)?;
    connection.busy_timeout(Duration::from_secs(5))?;
    let mode: String = connection.pragma_query_value(None, "journal_mode", |row| row.get(0))?;
    if !mode.eq_ignore_ascii_case("wal") {
        connection.pragma_update(None, "journal_mode", "WAL")?;
    }
    connection.pragma_update(None, "synchronous", "NORMAL")?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    let version = schema_version(&connection)?;
    if version > SCHEMA_VERSION {
        return Err(LearnError::NewerSchema(version));
    }
    if version < SCHEMA_VERSION {
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let version = schema_version(&transaction)?;
        if version > SCHEMA_VERSION {
            return Err(LearnError::NewerSchema(version));
        }
        for migration in MIGRATIONS
            .iter()
            .skip(usize::try_from(version).unwrap_or(0))
        {
            transaction.execute_batch(migration)?;
        }
        transaction.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        transaction.commit()?;
    }
    Ok(Learn { connection })
}

fn schema_version(connection: &Connection) -> Result<i64, LearnError> {
    Ok(connection.pragma_query_value(None, "user_version", |row| row.get(0))?)
}

fn is_corruption(error: &rusqlite::Error) -> bool {
    matches!(
        error.sqlite_error_code(),
        Some(ErrorCode::NotADatabase | ErrorCode::DatabaseCorrupt)
    )
}

fn set_aside(path: &Path) {
    let stamp = now();
    for suffix in ["", "-wal", "-shm"] {
        let mut from = path.as_os_str().to_owned();
        from.push(suffix);
        let mut to = path.as_os_str().to_owned();
        to.push(format!(".corrupt-{stamp}{suffix}"));
        let _ = fs::rename(PathBuf::from(from), PathBuf::from(to));
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::{Event, EventKind, LEARN_FILE, Learn, LearnError, MIGRATIONS, SCHEMA_VERSION};

    fn read(session: &str, at: i64, path: &str) -> Event {
        Event {
            path: Some(path.to_owned()),
            start_line: Some(1),
            end_line: Some(40),
            ..Event::new(session, at, EventKind::Read)
        }
    }

    #[test]
    fn records_events_linked_by_session() {
        let project = tempfile::tempdir().unwrap();
        let mut learn = Learn::open_project(project.path()).unwrap();
        let prompt = Event {
            text: Some("fix the login bug".to_owned()),
            ..Event::new("a", 10, EventKind::Prompt)
        };
        learn.record(&prompt).unwrap();
        learn.record(&read("a", 11, "src/auth.rs")).unwrap();
        learn.record(&read("b", 12, "src/other.rs")).unwrap();
        learn.record(&Event::new("a", 13, EventKind::Stop)).unwrap();

        let events = learn.events("a").unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0], prompt);
        assert_eq!(events[1].path.as_deref(), Some("src/auth.rs"));
        assert_eq!(events[2].kind, EventKind::Stop);
        let sessions = learn.sessions().unwrap();
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].started, 10);
        assert_eq!(sessions[0].ended, Some(13));
        assert_eq!(learn.session("b").unwrap().unwrap().ended, None);
        assert!(project.path().join(".wizard/.gitignore").is_file());
    }

    #[test]
    fn keeps_events_across_reopening() {
        let project = tempfile::tempdir().unwrap();
        Learn::open_project(project.path())
            .unwrap()
            .record(&read("a", 1, "x.rs"))
            .unwrap();
        let learn = Learn::open_project(project.path()).unwrap();
        assert_eq!(learn.events("a").unwrap().len(), 1);
    }

    #[test]
    fn migrates_an_older_store_and_keeps_its_events() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(LEARN_FILE);
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection.execute_batch(MIGRATIONS[0]).unwrap();
        connection
            .execute_batch(
                "INSERT INTO sessions (id, started) VALUES ('a', 1);
                 INSERT INTO events (session, at, kind, path) VALUES ('a', 1, 'read', 'x.rs');
                 PRAGMA user_version = 1;",
            )
            .unwrap();
        drop(connection);
        let mut learn = Learn::open(&path).unwrap();
        assert_eq!(learn.events("a").unwrap().len(), 1);
        learn.consolidate("a").unwrap();
        assert_eq!(learn.outcomes("a").unwrap().len(), 1);
    }

    #[test]
    fn leaves_a_newer_store_untouched() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(LEARN_FILE);
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection
            .pragma_update(None, "user_version", SCHEMA_VERSION + 1)
            .unwrap();
        drop(connection);
        assert!(matches!(
            Learn::open(&path),
            Err(LearnError::NewerSchema(version)) if version == SCHEMA_VERSION + 1
        ));
    }

    #[test]
    fn sets_a_corrupt_store_aside() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(LEARN_FILE);
        std::fs::write(&path, vec![b'x'; 4096]).unwrap();
        let mut learn = Learn::open(&path).unwrap();
        learn.record(&read("a", 1, "x.rs")).unwrap();
        let aside = std::fs::read_dir(directory.path())
            .unwrap()
            .filter_map(Result::ok)
            .any(|entry| entry.file_name().to_string_lossy().contains(".corrupt-"));
        assert!(aside);
    }
}
