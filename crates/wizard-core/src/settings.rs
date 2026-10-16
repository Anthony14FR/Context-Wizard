//! Per-project Claude Code settings in `.claude/settings.local.json`.
//!
//! Disabling the plugin there makes Claude Code unload its hooks, MCP server and skill for the
//! project. The file belongs to the user: unrelated keys and their order are kept, and a file
//! that is not a JSON object is never overwritten. A rewritten file is pretty-printed with
//! two-space indentation, and unchanged settings never cause a rewrite.

use std::error::Error;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::atomic;

/// Location of the local settings file, relative to the project root.
pub const RELATIVE_PATH: &str = ".claude/settings.local.json";

const ENABLED_PLUGINS: &str = "enabledPlugins";
const PERMISSIONS: &str = "permissions";
const ALLOW: &str = "allow";

/// Failure to update the local settings file.
#[derive(Debug)]
#[non_exhaustive]
pub enum SettingsError {
    /// The file could not be read or written.
    Io {
        /// Path that failed.
        path: PathBuf,
        /// Underlying error.
        source: io::Error,
    },
    /// The file is not valid JSON, for example because it contains comments.
    Invalid {
        /// Path of the file left untouched.
        path: PathBuf,
        /// Underlying error.
        source: serde_json::Error,
    },
    /// The file is not a JSON object, or `enabledPlugins` is not an object.
    NotAnObject {
        /// Path of the file left untouched.
        path: PathBuf,
    },
}

impl fmt::Display for SettingsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(formatter, "cannot access {}: {source}", path.display())
            }
            Self::Invalid { path, source } => write!(
                formatter,
                "{} is not valid JSON, left untouched: {source}",
                path.display()
            ),
            Self::NotAnObject { path } => write!(
                formatter,
                "{} is not a JSON object with an object `{ENABLED_PLUGINS}`, left untouched",
                path.display()
            ),
        }
    }
}

impl Error for SettingsError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Invalid { source, .. } => Some(source),
            Self::NotAnObject { .. } => None,
        }
    }
}

/// Disables `plugin_id` for `project`, or removes that override when `enabled` is `true`.
///
/// Enabling removes only a `false` override, so an explicit `true` set by the user is replaced
/// by `false` when disabling and is not restored afterwards.
///
/// # Errors
///
/// Returns an error when the file cannot be read or written, or does not hold a JSON object.
pub fn set_plugin_enabled(
    project: &Path,
    plugin_id: &str,
    enabled: bool,
) -> Result<(), SettingsError> {
    update(project, |settings, path| {
        if enabled {
            remove_override(settings, plugin_id, path)
        } else {
            add_override(settings, plugin_id, path)
        }
    })
}

/// Allows the tools matched by `rule` without a permission prompt in `project`, or removes
/// that rule when `allowed` is `false`. Other rules are left as they are.
///
/// # Errors
///
/// Returns an error when the file cannot be read or written, or does not hold a JSON object
/// with an object `permissions` and an array `allow`.
pub fn set_tools_allowed(project: &Path, rule: &str, allowed: bool) -> Result<(), SettingsError> {
    update(project, |settings, path| {
        let not_an_object = || SettingsError::NotAnObject {
            path: path.to_owned(),
        };
        if !allowed && !settings.contains_key(PERMISSIONS) {
            return Ok(false);
        }
        let permissions = settings
            .entry(PERMISSIONS)
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .ok_or_else(not_an_object)?;
        if !allowed && !permissions.contains_key(ALLOW) {
            return Ok(false);
        }
        let rules = permissions
            .entry(ALLOW)
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .ok_or_else(not_an_object)?;
        let present = rules.iter().any(|known| known.as_str() == Some(rule));
        if allowed == present {
            return Ok(false);
        }
        if allowed {
            rules.push(Value::String(rule.to_owned()));
        } else {
            rules.retain(|known| known.as_str() != Some(rule));
            if rules.is_empty() {
                permissions.shift_remove(ALLOW);
            }
            if permissions.is_empty() {
                settings.shift_remove(PERMISSIONS);
            }
        }
        Ok(true)
    })
}

fn update(
    project: &Path,
    change: impl FnOnce(&mut Map<String, Value>, &Path) -> Result<bool, SettingsError>,
) -> Result<(), SettingsError> {
    let path = project.join(RELATIVE_PATH);
    let mut settings = read_object(&path)?;
    if !change(&mut settings, &path)? {
        return Ok(());
    }
    let io_error = |source| SettingsError::Io {
        path: path.clone(),
        source,
    };
    let mut json = serde_json::to_string_pretty(&Value::Object(settings))
        .map_err(|error| io_error(io::Error::new(io::ErrorKind::InvalidData, error)))?;
    json.push('\n');
    atomic::write(&path, json.as_bytes()).map_err(io_error)
}

fn read_object(path: &Path) -> Result<Map<String, Value>, SettingsError> {
    match fs::read(path) {
        Ok(bytes) => {
            let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes);
            if bytes.iter().all(u8::is_ascii_whitespace) {
                return Ok(Map::new());
            }
            match serde_json::from_slice(bytes) {
                Ok(Value::Object(object)) => Ok(object),
                Ok(_) => Err(SettingsError::NotAnObject {
                    path: path.to_owned(),
                }),
                Err(source) => Err(SettingsError::Invalid {
                    path: path.to_owned(),
                    source,
                }),
            }
        }
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(Map::new()),
        Err(source) => Err(SettingsError::Io {
            path: path.to_owned(),
            source,
        }),
    }
}

fn add_override(
    settings: &mut Map<String, Value>,
    plugin_id: &str,
    path: &Path,
) -> Result<bool, SettingsError> {
    let plugins = settings
        .entry(ENABLED_PLUGINS)
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| SettingsError::NotAnObject {
            path: path.to_owned(),
        })?;
    let previous = plugins.insert(plugin_id.to_owned(), Value::Bool(false));
    Ok(previous != Some(Value::Bool(false)))
}

fn remove_override(
    settings: &mut Map<String, Value>,
    plugin_id: &str,
    path: &Path,
) -> Result<bool, SettingsError> {
    let Some(plugins) = settings.get_mut(ENABLED_PLUGINS) else {
        return Ok(false);
    };
    let plugins = plugins
        .as_object_mut()
        .ok_or_else(|| SettingsError::NotAnObject {
            path: path.to_owned(),
        })?;
    if plugins.shift_remove(plugin_id) != Some(Value::Bool(false)) {
        return Ok(false);
    }
    if plugins.is_empty() {
        settings.shift_remove(ENABLED_PLUGINS);
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use super::{RELATIVE_PATH, set_plugin_enabled, set_tools_allowed};

    #[test]
    fn adds_and_removes_one_allow_rule() {
        let project = tempfile::tempdir().unwrap();
        write(
            project.path(),
            r#"{"permissions":{"allow":["Bash(npm test)"]},"model":"x"}"#,
        );
        set_tools_allowed(project.path(), "mcp__x__*", true).unwrap();
        set_tools_allowed(project.path(), "mcp__x__*", true).unwrap();
        let value: serde_json::Value = serde_json::from_str(&read(project.path())).unwrap();
        assert_eq!(
            value["permissions"]["allow"],
            serde_json::json!(["Bash(npm test)", "mcp__x__*"])
        );
        set_tools_allowed(project.path(), "mcp__x__*", false).unwrap();
        let value: serde_json::Value = serde_json::from_str(&read(project.path())).unwrap();
        assert_eq!(
            value["permissions"]["allow"],
            serde_json::json!(["Bash(npm test)"])
        );
        assert_eq!(value["model"], "x");
        let empty = tempfile::tempdir().unwrap();
        set_tools_allowed(empty.path(), "mcp__x__*", false).unwrap();
        assert!(!empty.path().join(RELATIVE_PATH).exists());
    }

    const ID: &str = "context-wizard@context-wizard";

    fn read(project: &Path) -> String {
        fs::read_to_string(project.join(RELATIVE_PATH)).unwrap()
    }

    fn write(project: &Path, content: &str) {
        fs::create_dir_all(project.join(".claude")).unwrap();
        fs::write(project.join(RELATIVE_PATH), content).unwrap();
    }

    #[test]
    fn creates_the_override() {
        let project = tempfile::tempdir().unwrap();
        set_plugin_enabled(project.path(), ID, false).unwrap();
        assert_eq!(
            read(project.path()),
            "{\n  \"enabledPlugins\": {\n    \"context-wizard@context-wizard\": false\n  }\n}\n"
        );
    }

    #[test]
    fn keeps_unrelated_keys_in_order() {
        let project = tempfile::tempdir().unwrap();
        write(
            project.path(),
            r#"{"permissions":{"allow":["Bash(ls)"]},"enabledPlugins":{"other@market":true},"model":"x"}"#,
        );
        set_plugin_enabled(project.path(), ID, false).unwrap();
        let value: serde_json::Value = serde_json::from_str(&read(project.path())).unwrap();
        let keys: Vec<&String> = value.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["permissions", "enabledPlugins", "model"]);
        assert_eq!(value["enabledPlugins"]["other@market"], true);
        assert_eq!(value["enabledPlugins"][ID], false);
    }

    #[test]
    fn enabling_removes_only_our_override() {
        let project = tempfile::tempdir().unwrap();
        write(project.path(), r#"{"model":"x"}"#);
        set_plugin_enabled(project.path(), ID, false).unwrap();
        set_plugin_enabled(project.path(), ID, true).unwrap();
        let value: serde_json::Value = serde_json::from_str(&read(project.path())).unwrap();
        assert_eq!(value, serde_json::json!({ "model": "x" }));
    }

    #[test]
    fn enabling_without_file_writes_nothing() {
        let project = tempfile::tempdir().unwrap();
        set_plugin_enabled(project.path(), ID, true).unwrap();
        assert!(!project.path().join(RELATIVE_PATH).exists());
    }

    #[test]
    fn unchanged_settings_are_not_rewritten() {
        let project = tempfile::tempdir().unwrap();
        let disabled = r#"{ "enabledPlugins": { "context-wizard@context-wizard": false } }"#;
        let others =
            r#"{ "enabledPlugins": { "context-wizard@context-wizard": true, "x@y": true } }"#;
        write(project.path(), disabled);
        set_plugin_enabled(project.path(), ID, false).unwrap();
        assert_eq!(read(project.path()), disabled);
        write(project.path(), others);
        set_plugin_enabled(project.path(), ID, true).unwrap();
        assert_eq!(read(project.path()), others);
    }

    #[test]
    fn replaces_an_explicit_true_when_disabling() {
        let project = tempfile::tempdir().unwrap();
        write(
            project.path(),
            r#"{ "enabledPlugins": { "context-wizard@context-wizard": true } }"#,
        );
        set_plugin_enabled(project.path(), ID, false).unwrap();
        let value: serde_json::Value = serde_json::from_str(&read(project.path())).unwrap();
        assert_eq!(value["enabledPlugins"][ID], false);
    }

    #[test]
    fn accepts_empty_files_and_a_byte_order_mark() {
        let project = tempfile::tempdir().unwrap();
        for content in ["", "  \n", "\u{feff}{\"model\": \"x\"}"] {
            write(project.path(), content);
            set_plugin_enabled(project.path(), ID, false).unwrap();
            let value: serde_json::Value = serde_json::from_str(&read(project.path())).unwrap();
            assert_eq!(value["enabledPlugins"][ID], false);
        }
    }

    #[test]
    fn never_overwrites_a_file_it_cannot_read_as_an_object() {
        let project = tempfile::tempdir().unwrap();
        for content in ["[1, 2]", "{ broken", r#"{"enabledPlugins": []}"#] {
            write(project.path(), content);
            assert!(set_plugin_enabled(project.path(), ID, false).is_err());
            assert_eq!(read(project.path()), content);
        }
    }
}
