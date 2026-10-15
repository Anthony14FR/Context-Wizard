//! Per-project opt-in decision, stored in `.claude/wizard.json`.

use std::error::Error;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::atomic;
use crate::date::Date;

/// Schema version written and understood by this build.
pub const SCHEMA_VERSION: u64 = 1;

/// Location of the decision file, relative to the project root.
pub const RELATIVE_PATH: &str = ".claude/wizard.json";

/// Content of a decision file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Decision {
    /// Whether Context Wizard is enabled for the project.
    pub enabled: bool,
    /// Schema version of the file.
    pub version: u64,
    /// Day the decision was taken, formatted as `YYYY-MM-DD`.
    pub decided_at: String,
    /// Fields written by newer builds of the same schema version, kept on rewrite.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Decision {
    /// Creates a decision taken on `date`, with the current schema version.
    #[must_use]
    pub fn new(enabled: bool, date: Date) -> Self {
        Self {
            enabled,
            version: SCHEMA_VERSION,
            decided_at: date.to_string(),
            extra: Map::new(),
        }
    }
}

/// What a project's decision file says.
#[derive(Debug, Clone, PartialEq)]
pub enum State {
    /// No decision file: the opt-in question has not been answered yet.
    Undecided,
    /// The decision file enables or disables Context Wizard.
    Decided(Decision),
    /// The decision file uses a schema version newer than this build understands.
    Unsupported {
        /// Schema version found in the file.
        version: u64,
    },
}

impl State {
    /// Returns `true` only when the project explicitly enabled Context Wizard.
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        matches!(self, Self::Decided(decision) if decision.enabled)
    }
}

/// Failure to read or write a decision file.
#[derive(Debug)]
#[non_exhaustive]
pub enum DecisionError {
    /// The file or its directory could not be read or written.
    Io {
        /// Path that failed.
        path: PathBuf,
        /// Underlying error.
        source: io::Error,
    },
    /// The file is not valid JSON or does not match the schema.
    Malformed {
        /// Path of the malformed file.
        path: PathBuf,
        /// Underlying error.
        source: serde_json::Error,
    },
    /// The file is valid JSON but its `version` field is missing or not a positive integer.
    InvalidVersion {
        /// Path of the invalid file.
        path: PathBuf,
    },
}

impl fmt::Display for DecisionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(formatter, "cannot access {}: {source}", path.display())
            }
            Self::Malformed { path, source } => {
                write!(formatter, "malformed {}: {source}", path.display())
            }
            Self::InvalidVersion { path } => write!(
                formatter,
                "invalid {}: version must be a positive integer",
                path.display()
            ),
        }
    }
}

impl Error for DecisionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Malformed { source, .. } => Some(source),
            Self::InvalidVersion { .. } => None,
        }
    }
}

/// Returns the path of the decision file of `project`.
#[must_use]
pub fn path(project: &Path) -> PathBuf {
    project.join(RELATIVE_PATH)
}

/// Reads the decision of `project`.
///
/// A `version` above [`SCHEMA_VERSION`], including one too large for `u64`, yields
/// [`State::Unsupported`], whose `version` then saturates at `u64::MAX`.
///
/// # Errors
///
/// Returns an error when the file exists but cannot be read, is malformed, or has an invalid
/// `version`.
pub fn load(project: &Path) -> Result<State, DecisionError> {
    let path = path(project);
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(State::Undecided),
        Err(source) => return Err(DecisionError::Io { path, source }),
    };
    let malformed = |source| DecisionError::Malformed {
        path: path.clone(),
        source,
    };
    let value: Value = serde_json::from_slice(&bytes).map_err(malformed)?;
    match schema_version(&value) {
        Some(0) | None => Err(DecisionError::InvalidVersion { path }),
        Some(version) if version > SCHEMA_VERSION => Ok(State::Unsupported { version }),
        Some(_) => Ok(State::Decided(
            serde_json::from_value(value).map_err(malformed)?,
        )),
    }
}

fn schema_version(value: &Value) -> Option<u64> {
    let number = value.get("version")?.as_number()?;
    number.as_u64().or_else(|| {
        number
            .as_f64()
            .filter(|version| *version >= 18_446_744_073_709_551_616.0)
            .map(|_| u64::MAX)
    })
}

/// Records whether Context Wizard is enabled for `project`, taken on `date`.
///
/// Fields unknown to this build are kept when the existing file has the same schema version.
/// A malformed existing file is replaced. Concurrent decisions are last-writer-wins.
///
/// # Errors
///
/// Returns an error when the existing file cannot be read, or the new one cannot be written.
pub fn decide(project: &Path, enabled: bool, date: Date) -> Result<Decision, DecisionError> {
    let mut decision = Decision::new(enabled, date);
    match load(project) {
        Ok(State::Decided(previous)) => decision.extra = previous.extra,
        Ok(_) | Err(DecisionError::Malformed { .. } | DecisionError::InvalidVersion { .. }) => {}
        Err(error) => return Err(error),
    }
    store(project, &decision)?;
    Ok(decision)
}

/// Writes `decision` to the decision file of `project`, replacing it atomically.
///
/// # Errors
///
/// Returns an error when the directory or the file cannot be written.
pub fn store(project: &Path, decision: &Decision) -> Result<(), DecisionError> {
    let path = path(project);
    let mut json =
        serde_json::to_string_pretty(decision).map_err(|source| DecisionError::Malformed {
            path: path.clone(),
            source,
        })?;
    json.push('\n');
    atomic::write(&path, json.as_bytes()).map_err(|source| DecisionError::Io { path, source })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::json;
    use tempfile::TempDir;

    use super::{Decision, SCHEMA_VERSION, State, decide, load, path, store};
    use crate::date::Date;

    fn project() -> TempDir {
        tempfile::tempdir().unwrap()
    }

    fn day() -> Date {
        Date::from_unix_days(20_741)
    }

    fn write(project: &TempDir, content: &str) {
        let file = path(project.path());
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, content).unwrap();
    }

    #[test]
    fn missing_file_is_undecided() {
        assert_eq!(load(project().path()).unwrap(), State::Undecided);
    }

    #[test]
    fn writes_the_documented_format() {
        let project = project();
        decide(project.path(), true, day()).unwrap();
        let content = fs::read_to_string(path(project.path())).unwrap();
        assert_eq!(
            content,
            "{\n  \"enabled\": true,\n  \"version\": 1,\n  \"decidedAt\": \"2026-10-15\"\n}\n"
        );
    }

    #[test]
    fn round_trips_both_answers() {
        let project = project();
        for enabled in [true, false] {
            let decision = decide(project.path(), enabled, day()).unwrap();
            let state = load(project.path()).unwrap();
            assert_eq!(state, State::Decided(decision));
            assert_eq!(state.is_enabled(), enabled);
        }
    }

    #[test]
    fn newer_schema_is_unsupported() {
        let project = project();
        write(
            &project,
            r#"{ "enabled": true, "version": 2, "mode": "team" }"#,
        );
        let state = load(project.path()).unwrap();
        assert_eq!(state, State::Unsupported { version: 2 });
        assert!(!state.is_enabled());
    }

    #[test]
    fn unknown_fields_survive_a_new_decision() {
        let project = project();
        write(
            &project,
            r#"{ "enabled": false, "version": 1, "decidedAt": "2026-01-01", "team": "core" }"#,
        );
        let decision = decide(project.path(), true, day()).unwrap();
        assert_eq!(decision.extra.get("team"), Some(&json!("core")));
        assert!(load(project.path()).unwrap().is_enabled());
    }

    #[test]
    fn invalid_versions_are_errors() {
        let project = project();
        for version in ["0", "-1", "0.5", "\"1\"", "null"] {
            write(
                &project,
                &format!(
                    r#"{{ "enabled": true, "version": {version}, "decidedAt": "2026-10-15" }}"#
                ),
            );
            assert!(load(project.path()).is_err(), "version {version}");
        }
        write(
            &project,
            r#"{ "enabled": true, "decidedAt": "2026-10-15" }"#,
        );
        assert!(load(project.path()).is_err());
    }

    #[test]
    fn huge_version_is_unsupported() {
        let project = project();
        write(&project, r#"{ "enabled": true, "version": 1e30 }"#);
        assert_eq!(
            load(project.path()).unwrap(),
            State::Unsupported { version: u64::MAX }
        );
    }

    #[test]
    fn decide_replaces_a_malformed_file() {
        let project = project();
        write(&project, "{ not json");
        decide(project.path(), false, day()).unwrap();
        assert!(!load(project.path()).unwrap().is_enabled());
    }

    #[test]
    fn malformed_file_is_an_error() {
        let project = project();
        write(&project, "{ not json");
        assert!(load(project.path()).is_err());
        write(
            &project,
            r#"{ "enabled": "yes", "version": 1, "decidedAt": "2026-10-15" }"#,
        );
        assert!(load(project.path()).is_err());
    }

    #[test]
    fn store_leaves_no_temporary_file() {
        let project = project();
        store(project.path(), &Decision::new(true, day())).unwrap();
        let names: Vec<String> = fs::read_dir(project.path().join(".claude"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["wizard.json"]);
        assert_eq!(Decision::new(true, day()).version, SCHEMA_VERSION);
    }
}
