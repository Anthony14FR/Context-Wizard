//! The indexing pipeline: scan, change detection, extraction and storage.

use std::collections::BTreeSet;
use std::error::Error;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rayon::prelude::{IntoParallelRefIterator, ParallelIterator};
use wizard_core::secrets;
use wizard_parse::extract::{self, Tag, TagKind};
use wizard_parse::lexical::{self, Entry, EntryKind};
use wizard_scan::{ScanOptions, scan};
use wizard_store::{Changes, FileSymbols, Index, StoreError, SymbolKind, SymbolRecord};

use crate::recipes::Applied;
use crate::structure::Structure;
use crate::vocabulary::Vocabulary;

const HEAD_BYTES: usize = 256;

/// Settings of an index build.
#[derive(Debug, Clone, Default)]
pub struct BuildOptions {
    /// Base directory of the grammar cache; the platform cache directory when absent.
    pub grammar_cache: Option<PathBuf>,
    /// Limits applied while scanning.
    pub scan: ScanOptions,
}

/// Outcome of an index build.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BuildReport {
    /// Number of indexed files.
    pub files: usize,
    /// Files added, modified or removed since the previous build.
    pub changes: Changes,
    /// Number of re-extracted files that were tagged by their grammar.
    pub tagged: usize,
    /// Number of re-extracted files that fell back to lexical extraction.
    pub lexical: usize,
    /// Wall-clock duration of the build.
    pub elapsed: Duration,
}

/// Failure to build the index.
#[derive(Debug)]
#[non_exhaustive]
pub enum IndexError {
    /// The index database failed.
    Store(StoreError),
}

impl fmt::Display for IndexError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(error) => write!(formatter, "cannot update the index: {error}"),
        }
    }
}

impl Error for IndexError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Store(error) => Some(error),
        }
    }
}

impl From<StoreError> for IndexError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

/// Brings the index of `project` up to date and re-extracts the files that changed.
///
/// # Errors
///
/// Returns an error when the index database cannot be opened, read or written.
pub fn build(project: &Path, options: &BuildOptions) -> Result<BuildReport, IndexError> {
    let start = Instant::now();
    if let Some(cache) = &options.grammar_cache {
        let _ = wizard_parse::grammar::configure_cache(cache);
    }
    let report = scan(project, &options.scan);
    let mut index = Index::open_project(project)?;
    let changes = index.refresh(project, &report.files)?;
    let applied = Applied::detect(project, &report.files);
    let mut to_extract: BTreeSet<String> = changes
        .added
        .iter()
        .chain(&changes.modified)
        .cloned()
        .collect();
    let recipes_changed = applied.changed_since_last_build(project);
    if recipes_changed {
        to_extract.extend(
            report
                .files
                .iter()
                .filter(|file| applied.reads(&file.path))
                .map(|file| file.path.clone()),
        );
        to_extract.extend(index.facts()?.into_iter().map(|fact| fact.path));
    }
    let to_extract: Vec<String> = to_extract.into_iter().collect();
    let extracted: Vec<(FileSymbols, bool)> = to_extract
        .par_iter()
        .filter_map(|path| {
            let content = fs::read(project.join(path)).ok()?;
            let content = String::from_utf8_lossy(&content);
            let (mut file, tagged) = extract_file(path, &content);
            file.symbols.extend(applied.symbols(path, &content));
            Some((file, tagged))
        })
        .collect();
    let tagged = extracted.iter().filter(|(_, tagged)| *tagged).count();
    let lexical = extracted.len() - tagged;
    let files: Vec<FileSymbols> = extracted.into_iter().map(|(file, _)| file).collect();
    index.store_symbols(&files)?;
    if recipes_changed {
        applied.mark_built(project);
    }
    if let Ok(structure) = Structure::load(&mut index) {
        let _ = Vocabulary::load(&mut index, &structure.paths);
    }
    Ok(BuildReport {
        files: report.files.len(),
        changes,
        tagged,
        lexical,
        elapsed: start.elapsed(),
    })
}

/// Extracts the symbols of one file, and reports whether its grammar tagged it.
///
/// Environment files only yield their variable names, and every name and signature goes
/// through secret redaction before it is stored.
#[must_use]
pub fn extract_file(path: &str, content: &str) -> (FileSymbols, bool) {
    let head: String = content.chars().take(HEAD_BYTES).collect();
    let language = if secrets::is_env_file(path) {
        None
    } else {
        wizard_parse::detect_language(path, &head)
    };
    let tags = language.and_then(|language| extract::extract(language, content).ok());
    let tagged = tags.as_ref().is_some_and(|tags| !tags.is_empty());
    let symbols = match tags {
        Some(tags) if tagged => tags.into_iter().flat_map(from_tag).collect(),
        _ => lexical::extract(path, content)
            .into_iter()
            .map(|entry| from_entry(&entry))
            .collect(),
    };
    (
        FileSymbols {
            path: path.to_owned(),
            language: language.map(str::to_owned),
            symbols,
        },
        tagged,
    )
}

fn from_tag(tag: Tag) -> Vec<SymbolRecord> {
    let doc = (!tag.doc.is_empty()).then(|| SymbolRecord {
        line: tag.line,
        kind: SymbolKind::Doc,
        name: secrets::redact(&tag.doc).into_owned(),
        role: secrets::redact(&tag.name).into_owned(),
        end_line: tag.line,
        signature: String::new(),
        count: 1,
    });
    let symbol = SymbolRecord {
        line: tag.line,
        kind: match tag.kind {
            TagKind::Definition => SymbolKind::Definition,
            TagKind::Reference => SymbolKind::Reference,
        },
        name: secrets::redact(&tag.name).into_owned(),
        role: tag.role,
        end_line: tag.end_line,
        signature: secrets::redact(&tag.signature).into_owned(),
        count: 1,
    };
    std::iter::once(symbol).chain(doc).collect()
}

fn from_entry(entry: &Entry) -> SymbolRecord {
    SymbolRecord {
        line: entry.line,
        kind: match entry.kind {
            EntryKind::Heading => SymbolKind::Heading,
            EntryKind::Key => SymbolKind::Key,
            EntryKind::Identifier => SymbolKind::Identifier,
        },
        name: secrets::redact(&entry.text).into_owned(),
        role: String::new(),
        end_line: entry.line,
        signature: String::new(),
        count: entry.count,
    }
}
