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
