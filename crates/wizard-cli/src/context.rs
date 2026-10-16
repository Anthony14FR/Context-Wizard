//! Index refresh and context rendering shared by the hooks and the commands.

use std::path::Path;

use wizard_index::build::{BuildOptions, build};
use wizard_index::overview::overview;
use wizard_store::Index;

use crate::install;

/// Settings of index builds started by the binary, with grammars cached in the plugin data
/// directory when the binary runs from a plugin install.
pub(crate) fn build_options() -> BuildOptions {
    BuildOptions {
        grammar_cache: install::data_dir().map(|data| data.join("grammars")),
        ..BuildOptions::default()
    }
}

/// Brings the index of `project` up to date and renders its overview.
pub(crate) fn refreshed_overview(project: &Path) -> Option<String> {
    build(project, &build_options()).ok()?;
    let index = Index::open_project(project).ok()?;
    overview(project, &index).ok()
}
