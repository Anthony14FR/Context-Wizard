//! SQLite persistence of the Context Wizard index.
//!
//! The index is a cache of the project: a schema change or a corrupt database rebuilds it from
//! scratch. It records a BLAKE3 hash per file. A file whose size and modification time are
//! unchanged is not read again, unless that modification time was too close to the previous
//! scan to be trusted, and a file that was only touched is recognised by its unchanged hash.

use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rayon::prelude::{IntoParallelRefIterator, ParallelIterator};
use rusqlite::{Connection, ErrorCode, OptionalExtension, TransactionBehavior, params};
use wizard_scan::FileEntry;

mod symbols;

pub use symbols::{FileRecord, FileSymbols, LocatedSymbol, SymbolKind, SymbolRecord};

/// Version of the index schema. A different version on disk triggers a rebuild.
pub const SCHEMA_VERSION: i64 = 4;

/// Directory holding Context Wizard data, relative to the project root.
pub const DATA_DIRECTORY: &str = ".wizard";

/// File name of the index inside [`DATA_DIRECTORY`].
pub const INDEX_FILE: &str = "index.db";

const TIMESTAMP_GRANULARITY: Duration = Duration::from_secs(2);
const OPEN_ATTEMPTS: u32 = 50;
const OPEN_BACKOFF: Duration = Duration::from_millis(20);

const SCHEMA: &str = "
    CREATE TABLE files (
        path TEXT PRIMARY KEY NOT NULL,
        size INTEGER NOT NULL,
        modified INTEGER,
        hash BLOB NOT NULL,
        indexed_at INTEGER NOT NULL,
        language TEXT
    ) STRICT;
    CREATE TABLE symbols (
        path TEXT NOT NULL REFERENCES files (path) ON DELETE CASCADE,
        kind TEXT NOT NULL,
        name TEXT NOT NULL,
        role TEXT NOT NULL,
        line INTEGER NOT NULL,
        end_line INTEGER NOT NULL,
        signature TEXT NOT NULL,
        count INTEGER NOT NULL
    ) STRICT;
    CREATE INDEX symbols_by_path ON symbols (path);
    CREATE INDEX symbols_by_name ON symbols (name);
    CREATE TABLE cache (
        name TEXT PRIMARY KEY NOT NULL,
        value BLOB NOT NULL
    ) STRICT;
";

/// Failure to open or update the index.
#[derive(Debug)]
#[non_exhaustive]
pub enum StoreError {
    /// The data directory could not be created.
    Io {
        /// Path that failed.
        path: PathBuf,
        /// Underlying error.
        source: io::Error,
    },
    /// SQLite reported an error.
    Sqlite(rusqlite::Error),
}

impl fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(formatter, "cannot prepare {}: {source}", path.display())
            }
            Self::Sqlite(source) => write!(formatter, "index database error: {source}"),
        }
    }
}

impl Error for StoreError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Sqlite(source) => Some(source),
        }
    }
}

impl From<rusqlite::Error> for StoreError {
    fn from(source: rusqlite::Error) -> Self {
        Self::Sqlite(source)
    }
}

/// Returns the `.wizard` directory of `project`, creating it when missing.
///
/// Callers check the project's opt-in decision first. The directory gets a `.gitignore` that
/// ignores everything, so its data is never committed even when the project does not ignore it.
///
/// # Errors
///
/// Returns an error when the directory or its `.gitignore` cannot be created.
pub fn data_directory(project: &Path) -> Result<PathBuf, StoreError> {
    let directory = project.join(DATA_DIRECTORY);
    let io_error = |source| StoreError::Io {
        path: directory.clone(),
        source,
    };
    fs::create_dir_all(&directory).map_err(io_error)?;
    let gitignore = directory.join(".gitignore");
    if !gitignore.exists() {
        fs::write(&gitignore, "*\n").map_err(io_error)?;
    }
    Ok(directory)
}

/// Files that differ between the index and the latest scan.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Changes {
    /// Files absent from the index, sorted.
    pub added: Vec<String>,
    /// Files whose content changed, sorted.
    pub modified: Vec<String>,
    /// Files no longer present, sorted.
    pub removed: Vec<String>,
    /// Number of files whose content is unchanged.
    pub unchanged: usize,
}

impl Changes {
    /// Returns `true` when no file was added, modified or removed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.modified.is_empty() && self.removed.is_empty()
    }
}

struct Stored {
    size: i64,
    modified: Option<i64>,
    hash: Vec<u8>,
    indexed_at: i64,
}

struct Update {
    path: String,
    size: i64,
    modified: Option<i64>,
    hash: [u8; 32],
}

/// The index of one project.
#[derive(Debug)]
pub struct Index {
    connection: Connection,
}

impl Index {
    /// Opens the index of `project` in its `.wizard` directory, creating both when missing.
    ///
    /// Callers check the project's opt-in decision first: this creates the directory. The
    /// directory gets a `.gitignore` that ignores everything, so the index is never committed
    /// even when the project does not ignore it.
    ///
    /// # Errors
    ///
    /// Returns an error when the directory cannot be created or the database cannot be opened.
    pub fn open_project(project: &Path) -> Result<Self, StoreError> {
        Self::open(&data_directory(project)?.join(INDEX_FILE))
    }

    /// Opens the index stored at `path`, rebuilding it when its schema version differs or when
    /// the file is not a valid database.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot be opened or initialised.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        match connect(path) {
            Err(StoreError::Sqlite(error)) if is_corruption(&error) => {
                for suffix in ["", "-wal", "-shm"] {
                    let mut file = path.as_os_str().to_owned();
                    file.push(suffix);
                    let _ = fs::remove_file(PathBuf::from(file));
                }
                connect(path)
            }
            result => result,
        }
    }

    /// Number of files in the index.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot be queried.
    pub fn len(&self) -> Result<usize, StoreError> {
        let count: i64 = self
            .connection
            .query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0))?;
        Ok(usize::try_from(count).unwrap_or(0))
    }

    /// Returns `true` when the index holds no file.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot be queried.
    pub fn is_empty(&self) -> Result<bool, StoreError> {
        Ok(self.len()? == 0)
    }

    /// Returns the BLAKE3 hash recorded for `path`.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot be queried.
    pub fn hash(&self, path: &str) -> Result<Option<[u8; 32]>, StoreError> {
        let hash: Option<Vec<u8>> = self
            .connection
            .query_row("SELECT hash FROM files WHERE path = ?1", [path], |row| {
                row.get(0)
            })
            .optional()?;
        Ok(hash.and_then(|hash| hash.try_into().ok()))
    }

    /// Returns the cached value `name`, computed from the index and dropped whenever a file
    /// or its symbols change.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot be queried.
    pub fn cached(&self, name: &str) -> Result<Option<Vec<u8>>, StoreError> {
        Ok(self
            .connection
            .query_row("SELECT value FROM cache WHERE name = ?1", [name], |row| {
                row.get(0)
            })
            .optional()?)
    }

    /// Stores `value` as the cached value `name`.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot be written.
    pub fn set_cached(&mut self, name: &str, value: &[u8]) -> Result<(), StoreError> {
        self.connection.execute(
            "INSERT INTO cache (name, value) VALUES (?1, ?2)
             ON CONFLICT (name) DO UPDATE SET value = excluded.value",
            rusqlite::params![name, value],
        )?;
        Ok(())
    }

    /// Brings the index in line with `files`, the latest scan of `root`, and reports what changed.
    ///
    /// Files that cannot be read while hashing are left out and reported as removed; the next
    /// refresh adds them back once they are readable.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot be read or written.
    pub fn refresh(&mut self, root: &Path, files: &[FileEntry]) -> Result<Changes, StoreError> {
        let scanned_at = nanoseconds(SystemTime::now()).unwrap_or(0);
        let mut stored = self.stored()?;
        let mut changes = Changes::default();
        let mut candidates = Vec::new();
        for file in files {
            let size = i64::try_from(file.size).unwrap_or(i64::MAX);
            let modified = file.modified.and_then(nanoseconds);
            if stored
                .get(&file.path)
                .is_some_and(|previous| is_stable(previous, size, modified))
            {
                changes.unchanged += 1;
                stored.remove(&file.path);
            } else {
                candidates.push((file.path.as_str(), size, modified));
            }
        }
        let hashed: Vec<Update> = candidates
            .par_iter()
            .filter_map(|(path, size, modified)| {
                Some(Update {
                    path: (*path).to_owned(),
                    size: *size,
                    modified: *modified,
                    hash: hash_file(&root.join(path))?,
                })
            })
            .collect();
        for update in &hashed {
            match stored.remove(&update.path) {
                None => changes.added.push(update.path.clone()),
                Some(previous) if previous.hash != update.hash => {
                    changes.modified.push(update.path.clone());
                }
                Some(_) => changes.unchanged += 1,
            }
        }
        changes.removed = stored.into_keys().collect();
        changes.added.sort_unstable();
        changes.modified.sort_unstable();
        changes.removed.sort_unstable();
        self.write(&hashed, &changes.removed, scanned_at)?;
        Ok(changes)
    }

    fn stored(&self) -> Result<HashMap<String, Stored>, StoreError> {
        let mut statement = self
            .connection
            .prepare("SELECT path, size, modified, hash, indexed_at FROM files")?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                Stored {
                    size: row.get(1)?,
                    modified: row.get(2)?,
                    hash: row.get(3)?,
                    indexed_at: row.get(4)?,
                },
            ))
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    fn write(
        &mut self,
        updates: &[Update],
        removed: &[String],
        scanned_at: i64,
    ) -> Result<(), StoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        {
            let mut upsert = transaction.prepare(
                "INSERT INTO files (path, size, modified, hash, indexed_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT (path) DO UPDATE SET
                     size = excluded.size, modified = excluded.modified,
                     hash = excluded.hash, indexed_at = excluded.indexed_at",
            )?;
            for update in updates {
                upsert.execute(params![
                    update.path,
                    update.size,
                    update.modified,
                    update.hash.as_slice(),
                    scanned_at
                ])?;
            }
            let mut delete = transaction.prepare("DELETE FROM files WHERE path = ?1")?;
            for path in removed {
                delete.execute([path])?;
            }
        }
        if !updates.is_empty() || !removed.is_empty() {
            transaction.execute("DELETE FROM cache", [])?;
        }
        transaction.commit()?;
        Ok(())
    }
}

fn is_stable(previous: &Stored, size: i64, modified: Option<i64>) -> bool {
    let granularity = i64::try_from(TIMESTAMP_GRANULARITY.as_nanos()).unwrap_or(i64::MAX);
    previous.size == size
        && previous.modified == modified
        && modified.is_some_and(|time| time.saturating_add(granularity) < previous.indexed_at)
}

fn hash_file(path: &Path) -> Option<[u8; 32]> {
    let file = fs::File::open(path).ok()?;
    let mut hasher = blake3::Hasher::new();
    hasher.update_reader(file).ok()?;
    Some(*hasher.finalize().as_bytes())
}

fn connect(path: &Path) -> Result<Index, StoreError> {
    let mut attempt = 1;
    loop {
        match try_connect(path) {
            Err(StoreError::Sqlite(error))
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

fn try_connect(path: &Path) -> Result<Index, StoreError> {
    let mut connection = Connection::open(path)?;
    connection.busy_timeout(Duration::from_secs(5))?;
    let mode: String = connection.pragma_query_value(None, "journal_mode", |row| row.get(0))?;
    if !mode.eq_ignore_ascii_case("wal") {
        connection.pragma_update(None, "journal_mode", "WAL")?;
    }
    connection.pragma_update(None, "synchronous", "NORMAL")?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    if schema_version(&connection)? != SCHEMA_VERSION {
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if schema_version(&transaction)? != SCHEMA_VERSION {
            transaction
                .execute_batch("DROP TABLE IF EXISTS cache; DROP TABLE IF EXISTS symbols; DROP TABLE IF EXISTS files;")?;
            transaction.execute_batch(SCHEMA)?;
            transaction.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        }
        transaction.commit()?;
    }
    Ok(Index { connection })
}

fn schema_version(connection: &Connection) -> Result<i64, StoreError> {
    Ok(connection.pragma_query_value(None, "user_version", |row| row.get(0))?)
}

fn is_corruption(error: &rusqlite::Error) -> bool {
    matches!(
        error.sqlite_error_code(),
        Some(ErrorCode::NotADatabase | ErrorCode::DatabaseCorrupt)
    )
}

fn nanoseconds(time: SystemTime) -> Option<i64> {
    let elapsed = time.duration_since(UNIX_EPOCH).ok()?;
    i64::try_from(elapsed.as_nanos()).ok()
}
