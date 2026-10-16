//! Answers to the questions Claude asks through the MCP tools.
//!
//! Every answer is compact Markdown with paths and 1-based line numbers, sorted so the same
//! index always gives the same text, and capped so a broad question stays cheap.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use wizard_git::History;
use wizard_store::{Index, LocatedSymbol, StoreError, SymbolKind};

use crate::graph::FileGraph;

const MAX_MATCHES: usize = 20;
const MAX_FILES: usize = 15;
const MAX_LINES_PER_FILE: usize = 12;
const MAX_SLICE_FILES: usize = 10;
const MIN_KEYWORD_LENGTH: usize = 3;
const STOP_WORDS: [&str; 20] = [
    "the", "and", "for", "with", "that", "this", "from", "into", "when", "where", "what", "add",
    "fix", "make", "use", "new", "all", "how", "should", "does",
];

/// Definitions named `name`, or, when none matches exactly, definitions whose name contains it.
///
/// # Errors
///
/// Returns an error when the index cannot be read.
pub fn find_symbol(index: &Index, name: &str) -> Result<String, StoreError> {
    let exact: Vec<LocatedSymbol> = index
        .symbols_named(name)?
        .into_iter()
        .filter(|located| located.symbol.kind == SymbolKind::Definition)
        .collect();
    let (heading, found) = if exact.is_empty() {
        let needle = name.to_lowercase();
        let similar: Vec<LocatedSymbol> = index
            .symbols()?
            .into_iter()
            .filter(|located| {
                located.symbol.kind == SymbolKind::Definition
                    && located.symbol.name.to_lowercase().contains(&needle)
            })
            .collect();
        (
            format!("No definition named `{name}`. Similar names:"),
            similar,
        )
    } else {
        (format!("Definitions of `{name}`:"), exact)
    };
    if found.is_empty() {
        return Ok(format!("No definition matches `{name}`.\n"));
    }
    let mut text = format!("{heading}\n");
    for located in found.iter().take(MAX_MATCHES) {
        let _ = writeln!(
            text,
            "- {}:{} {}",
            located.path,
            located.symbol.line,
            describe(located)
        );
    }
    push_remainder(&mut text, found.len(), MAX_MATCHES);
    Ok(text)
}

/// Where `name` is used, grouped by file, with its definitions first.
///
/// # Errors
///
/// Returns an error when the index cannot be read.
pub fn refs(index: &Index, name: &str) -> Result<String, StoreError> {
    let symbols = index.symbols_named(name)?;
    let definitions: Vec<&LocatedSymbol> = symbols
        .iter()
        .filter(|located| located.symbol.kind == SymbolKind::Definition)
        .collect();
    let mut uses: BTreeMap<&str, Vec<u32>> = BTreeMap::new();
    for located in &symbols {
        if located.symbol.kind == SymbolKind::Reference {
            uses.entry(located.path.as_str())
                .or_default()
                .push(located.symbol.line);
        }
    }
    if definitions.is_empty() && uses.is_empty() {
        return Ok(format!(
            "No definition or reference of `{name}` is indexed.\n"
        ));
    }
    let mut text = String::new();
    if !definitions.is_empty() {
        let _ = writeln!(text, "Defined in:");
        for located in definitions.iter().take(MAX_MATCHES) {
            let _ = writeln!(
                text,
                "- {}:{} {}",
                located.path,
                located.symbol.line,
                describe(located)
            );
        }
    }
    let _ = writeln!(text, "Used in {} files:", uses.len());
    for (path, lines) in uses.iter().take(MAX_FILES) {
        let _ = writeln!(text, "- {path}: lines {}", join_lines(lines));
    }
    push_remainder(&mut text, uses.len(), MAX_FILES);
    Ok(text)
}

/// What a file defines, what it uses and who uses it.
///
/// # Errors
///
/// Returns an error when the index cannot be read.
pub fn explain(index: &Index, path: &str) -> Result<String, StoreError> {
    let own = index.symbols_of(path)?;
    let Some(record) = index.files()?.into_iter().find(|file| file.path == path) else {
        return Ok(format!(
            "`{path}` is not indexed. Paths are relative to the project root, with `/`.\n"
        ));
    };
    let symbols = index.symbols()?;
    let graph = FileGraph::new(&symbols, None);
    let language = record.language.as_deref().unwrap_or("unknown language");
    let mut text = format!("# {path} ({language}, {} bytes)\n", record.size);
    let outline: Vec<&LocatedSymbol> = own
        .iter()
        .filter(|located| located.symbol.kind != SymbolKind::Reference)
        .collect();
    if !outline.is_empty() {
        let _ = writeln!(text, "Outline:");
        for located in outline.iter().take(MAX_LINES_PER_FILE * 3) {
            let _ = writeln!(text, "{:>6} {}", located.symbol.line, describe(located));
        }
        push_remainder(&mut text, outline.len(), MAX_LINES_PER_FILE * 3);
    }
    if let Some(node) = graph.position(path) {
        list_files(&mut text, "Uses", &graph, &graph.dependencies[node]);
        list_files(&mut text, "Used by", &graph, &graph.referrers[node]);
    }
    Ok(text)
}

/// Files likely affected by a change to `path`: direct and indirect dependents, and the files
/// that usually change with it.
///
/// # Errors
///
/// Returns an error when the index cannot be read.
pub fn impact(index: &Index, path: &str, history: Option<&History>) -> Result<String, StoreError> {
    let symbols = index.symbols()?;
    let graph = FileGraph::new(&symbols, history);
    let Some(node) = graph.position(path) else {
        return Ok(format!(
            "`{path}` has no indexed symbols. Paths are relative to the project root, with `/`.\n"
        ));
    };
    let direct = &graph.referrers[node];
    let indirect: BTreeSet<usize> = direct
        .iter()
        .flat_map(|referrer| graph.referrers[*referrer].iter().copied())
        .filter(|candidate| *candidate != node && !direct.contains(candidate))
        .collect();
    let mut text = format!("# Impact of a change to {path}\n");
    list_files(&mut text, "Direct dependents", &graph, direct);
    list_files(&mut text, "Indirect dependents", &graph, &indirect);
    if let Some(history) = history {
        let mut partners: Vec<(&str, u32)> = history
            .co_change
            .iter()
            .filter_map(|((left, right), times)| {
                if left == path {
                    Some((right.as_str(), *times))
                } else if right == path {
                    Some((left.as_str(), *times))
                } else {
                    None
                }
            })
            .collect();
        partners.sort_by(|left, right| right.1.cmp(&left.1).then(left.0.cmp(right.0)));
        if !partners.is_empty() {
            let _ = writeln!(text, "Usually changes with:");
            for (partner, times) in partners.iter().take(MAX_FILES) {
                let _ = writeln!(text, "- {partner} ({times} commits)");
            }
        }
    }
    Ok(text)
}

/// Files most relevant to `task`, a description in natural language, with their matching
/// definitions.
///
/// Files whose symbols or path contain the task's keywords seed a personalised `PageRank` over
/// the file graph, so central files close to those matches rank high as well.
///
/// # Errors
///
/// Returns an error when the index cannot be read.
pub fn slice(index: &Index, task: &str, history: Option<&History>) -> Result<String, StoreError> {
    let keywords = keywords(task);
    if keywords.is_empty() {
        return Ok("Describe the task with a few specific words, such as names of features, modules or symbols.\n".to_owned());
    }
    let symbols = index.symbols()?;
    let graph = FileGraph::new(&symbols, history);
    let mut seeds: BTreeMap<usize, f64> = BTreeMap::new();
    let mut matches: BTreeMap<usize, Vec<&LocatedSymbol>> = BTreeMap::new();
    for located in &symbols {
        let Some(node) = graph.position(&located.path) else {
            continue;
        };
        if located.symbol.kind != SymbolKind::Reference
            && keywords
                .iter()
                .any(|keyword| located.symbol.name.to_lowercase().contains(keyword))
        {
            *seeds.entry(node).or_default() += 1.0;
            matches.entry(node).or_default().push(located);
        }
    }
    for (node, path) in graph.paths.iter().enumerate() {
        let lower = path.to_lowercase();
        let hits = keywords
            .iter()
            .filter(|keyword| lower.contains(keyword.as_str()))
            .count();
        if hits > 0 {
            *seeds.entry(node).or_default() +=
                2.0 * f64::from(u32::try_from(hits).unwrap_or(u32::MAX));
        }
    }
    if seeds.is_empty() {
        return Ok(format!(
            "Nothing in the index matches {}. Try other words, or `find_symbol` with a name.\n",
            keywords.join(", ")
        ));
    }
    let ranks = graph.rank(&seeds);
    let mut order: Vec<usize> = (0..graph.paths.len()).collect();
    order.sort_by(|left, right| {
        ranks[*right]
            .partial_cmp(&ranks[*left])
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(graph.paths[*left].cmp(&graph.paths[*right]))
    });
    let mut text = format!("Files most relevant to: {}\n", keywords.join(", "));
    for node in order.into_iter().take(MAX_SLICE_FILES) {
        let _ = writeln!(text, "\n{}", graph.paths[node]);
        if let Some(found) = matches.get(&node) {
            for located in found.iter().take(MAX_LINES_PER_FILE) {
                let _ = writeln!(text, "{:>6} {}", located.symbol.line, describe(located));
            }
        }
    }
    Ok(text)
}

fn keywords(task: &str) -> Vec<String> {
    let mut words: BTreeSet<String> = BTreeSet::new();
    for word in task.split(|character: char| !character.is_alphanumeric()) {
        for part in split_identifier(word) {
            let lower = part.to_lowercase();
            if lower.chars().count() >= MIN_KEYWORD_LENGTH && !STOP_WORDS.contains(&lower.as_str())
            {
                words.insert(lower);
            }
        }
    }
    words.into_iter().collect()
}

fn split_identifier(word: &str) -> Vec<String> {
    let mut parts = vec![word.to_owned()];
    let mut current = String::new();
    let mut previous_lower = false;
    for character in word.chars() {
        if character.is_uppercase() && previous_lower && !current.is_empty() {
            parts.push(std::mem::take(&mut current));
        }
        previous_lower = character.is_lowercase();
        current.push(character);
    }
    if !current.is_empty() && current != word {
        parts.push(current);
    }
    parts
}

fn list_files(text: &mut String, title: &str, graph: &FileGraph, nodes: &BTreeSet<usize>) {
    if nodes.is_empty() {
        return;
    }
    let _ = writeln!(text, "{title} ({}):", nodes.len());
    for node in nodes.iter().take(MAX_FILES) {
        let _ = writeln!(text, "- {}", graph.paths[*node]);
    }
    push_remainder(text, nodes.len(), MAX_FILES);
}

fn describe(located: &LocatedSymbol) -> String {
    let symbol = &located.symbol;
    match symbol.kind {
        SymbolKind::Definition if !symbol.signature.is_empty() => symbol.signature.clone(),
        SymbolKind::Definition | SymbolKind::Reference => {
            format!("{} {}", symbol.role, symbol.name)
        }
        SymbolKind::Heading | SymbolKind::Key => symbol.name.clone(),
        SymbolKind::Identifier => format!("{} ({} uses)", symbol.name, symbol.count),
    }
}

fn join_lines(lines: &[u32]) -> String {
    let mut unique: Vec<u32> = lines.to_vec();
    unique.sort_unstable();
    unique.dedup();
    let shown: Vec<String> = unique
        .iter()
        .take(MAX_LINES_PER_FILE)
        .map(u32::to_string)
        .collect();
    let mut text = shown.join(", ");
    if unique.len() > MAX_LINES_PER_FILE {
        let _ = write!(text, " and {} more", unique.len() - MAX_LINES_PER_FILE);
    }
    text
}

fn push_remainder(text: &mut String, total: usize, shown: usize) {
    if total > shown {
        let _ = writeln!(text, "- and {} more", total - shown);
    }
}

#[cfg(test)]
mod tests {
    use super::keywords;

    #[test]
    fn keywords_split_identifiers_and_drop_noise() {
        assert_eq!(
            keywords("Fix the sessionStore refresh when login fails"),
            [
                "fails",
                "login",
                "refresh",
                "session",
                "sessionstore",
                "store"
            ]
        );
    }
}
