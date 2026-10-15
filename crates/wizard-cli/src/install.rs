//! Location of the plugin installation this binary runs from.

use std::env;
use std::path::PathBuf;

/// Name of the plugin, the first part of its `<plugin>@<marketplace>` id.
pub(crate) const PLUGIN_NAME: &str = "context-wizard";

/// Returns the plugin data directory when the binary runs from its `bin` directory.
///
/// Claude Code names that directory `<plugin>-<marketplace>`.
pub(crate) fn data_dir() -> Option<PathBuf> {
    let binary = env::current_exe().ok()?;
    let binary = binary.canonicalize().unwrap_or(binary);
    let bin = binary.parent()?;
    if bin.file_name()? != "bin" {
        return None;
    }
    let data = bin.parent()?;
    marketplace_from_data_dir(data.file_name()?.to_str()?)?;
    Some(data.to_path_buf())
}

/// Returns the `<plugin>@<marketplace>` id of the installed plugin.
pub(crate) fn plugin_id() -> Option<String> {
    let data = data_dir()?;
    let marketplace = marketplace_from_data_dir(data.file_name()?.to_str()?)?;
    Some(format!("{PLUGIN_NAME}@{marketplace}"))
}

/// Returns `true` when the plugin that launched this binary declares a different version.
///
/// The plugin and the binary are released together, so a mismatch means the plugin was updated
/// and its binary is not installed yet: hooks then stay inert until the launcher installs it.
/// Without `CLAUDE_PLUGIN_ROOT`, as when the binary is run by hand, there is no mismatch.
pub(crate) fn version_mismatch() -> bool {
    let Some(root) = env::var_os("CLAUDE_PLUGIN_ROOT").filter(|root| !root.is_empty()) else {
        return false;
    };
    let manifest = PathBuf::from(root)
        .join(".claude-plugin")
        .join("plugin.json");
    let declared = std::fs::read_to_string(manifest)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|manifest| manifest.get("version")?.as_str().map(str::to_owned));
    declared.is_some_and(|version| version != wizard_core::VERSION)
}

fn marketplace_from_data_dir(name: &str) -> Option<&str> {
    name.strip_prefix(PLUGIN_NAME)?
        .strip_prefix('-')
        .filter(|marketplace| !marketplace.is_empty())
}

#[cfg(test)]
mod tests {
    use super::marketplace_from_data_dir;

    #[test]
    fn reads_the_marketplace_from_the_data_directory() {
        assert_eq!(
            marketplace_from_data_dir("context-wizard-context-wizard"),
            Some("context-wizard")
        );
        assert_eq!(
            marketplace_from_data_dir("context-wizard-context-wizard-beta"),
            Some("context-wizard-beta")
        );
        assert_eq!(
            marketplace_from_data_dir("context-wizard-inline"),
            Some("inline")
        );
        assert_eq!(marketplace_from_data_dir("debug"), None);
        assert_eq!(marketplace_from_data_dir("context-wizard"), None);
    }
}
