//! Exact references through a language server, with name-based references as the fallback.

use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use std::time::Duration;

use wizard_lsp::{LspError, by_file, find_server, references};
use wizard_store::{Index, StoreError, SymbolKind};

use crate::query;

/// Time a language server gets to index the project and answer.
pub const LSP_TIMEOUT: Duration = Duration::from_secs(60);

/// Exact references of the definition `name`, in `path` when given, from the language server
/// of its language, or the name-based references of the index when no server is installed.
///
/// # Errors
///
/// Returns an error when the index cannot be read.
pub fn exact_refs(
    project: &Path,
    index: &Index,
    name: &str,
    path: Option<&str>,
    timeout: Duration,
) -> Result<String, StoreError> {
    let definitions: Vec<_> = index
        .symbols_named(name)?
        .into_iter()
        .filter(|located| located.symbol.kind == SymbolKind::Definition)
        .filter(|located| path.is_none_or(|path| located.path == path))
        .collect();
    let Some(definition) = definitions.first() else {
        return Ok(format!(
            "No definition named `{name}`{}. Check the name with `find_symbol`.\n",
            path.map(|path| format!(" in {path}")).unwrap_or_default()
        ));
    };
    let mut text = String::new();
    if definitions.len() > 1 {
        let _ = writeln!(
            text,
            "`{name}` is defined in {} places; this is the one in {}. Pass `path` to choose another.",
            definitions.len(),
            definition.path
        );
    }
    let language = index
        .files()?
        .into_iter()
        .find(|file| file.path == definition.path)
        .and_then(|file| file.language)
        .unwrap_or_default();
    let file = project.join(&definition.path);
    let line = definition.symbol.line.saturating_sub(1);
    let column = fs::read_to_string(&file)
        .ok()
        .and_then(|content| {
            let source = content.lines().nth(usize::try_from(line).ok()?)?.to_owned();
            let byte = source.find(name)?;
            u32::try_from(source[..byte].encode_utf16().count()).ok()
        })
        .unwrap_or(0);
    match references(project, &file, &language, line, column, timeout) {
        Ok(found) if found.is_empty() => {
            let _ = writeln!(
                text,
                "The language server returned no reference within {} seconds: it may still be indexing, or it could not load the project.",
                timeout.as_secs()
            );
            text.push_str(
                "Name-based references from the index:
",
            );
            text.push_str(&query::refs(index, name)?);
            Ok(text)
        }
        Ok(found) => {
            let server =
                find_server(&language).map_or("the language server", |(server, _)| server.command);
            let files = by_file(project, &found);
            let total: usize = files.values().map(Vec::len).sum();
            let _ = writeln!(
                text,
                "Exact references of `{name}` defined at {}:{} ({total} in {} files, from {server}):",
                definition.path,
                definition.symbol.line,
                files.len()
            );
            for (path, lines) in &files {
                let lines: Vec<String> = lines.iter().map(ToString::to_string).collect();
                let _ = writeln!(text, "- {path}: lines {}", lines.join(", "));
            }
            Ok(text)
        }
        Err(error @ (LspError::NotInstalled { .. } | LspError::Unsupported(_))) => {
            let _ = writeln!(text, "Exact references are unavailable: {error}.");
            text.push_str("Name-based references from the index, which may include other symbols with the same name:\n");
            text.push_str(&query::refs(index, name)?);
            Ok(text)
        }
        Err(error) => {
            let _ = writeln!(text, "The language server failed: {error}.");
            text.push_str("Name-based references from the index:\n");
            text.push_str(&query::refs(index, name)?);
            Ok(text)
        }
    }
}
