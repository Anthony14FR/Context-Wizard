//! Answers to the questions Claude asks through the MCP tools.
//!
//! Every answer is compact Markdown with paths and 1-based line numbers, sorted so the same
//! index always gives the same text, and capped so a broad question stays cheap.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

use wizard_core::text::{keywords, stems};
use wizard_git::History;
use wizard_store::{Index, LocatedSymbol, StoreError, SymbolKind};

use crate::graph::FileGraph;
use crate::learned::Learned;
use crate::map::{TEST_PENALTY, is_test_path};
use crate::recipes;
use crate::structure::lift;
use crate::vocabulary::Vocabulary;

const MAX_MATCHES: usize = 20;
const MAX_FILES: usize = 15;
const MAX_LINES_PER_FILE: usize = 12;
const MAX_SLICE_FILES: usize = 10;
const MAX_FACTS_PER_KIND: usize = 80;

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
    let graph = FileGraph::new(&symbols, None, &Learned::default());
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
pub fn impact(
    index: &Index,
    path: &str,
    history: Option<&History>,
    learned: &Learned,
) -> Result<String, StoreError> {
    let symbols = index.symbols()?;
    let graph = FileGraph::new(&symbols, history, learned);
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
    let opened: Vec<&str> = learned
        .partners(path)
        .take(MAX_FILES)
        .map(|(partner, _)| partner)
        .collect();
    if !opened.is_empty() {
        let _ = writeln!(text, "Usually opened with, in past sessions:");
        for partner in opened {
            let _ = writeln!(text, "- {partner}");
        }
    }
    Ok(text)
}

/// Files most relevant to `task`, a description in natural language, with their matching
/// definitions.
///
/// Files whose symbols or path contain the task's keywords seed a personalised `PageRank` over
/// the file graph, so files close to those matches rank high as well. Files rank by lift over
/// their global rank, so hubs do not crowd out the task, and tests rank after production code
/// unless the task is about tests.
///
/// # Errors
///
/// Returns an error when the index cannot be read.
pub fn slice(
    index: &Index,
    task: &str,
    history: Option<&History>,
    learned: &Learned,
) -> Result<String, StoreError> {
    let keywords = keywords(task);
    if keywords.is_empty() {
        return Ok("Describe the task with a few specific words, such as names of features, modules or symbols.\n".to_owned());
    }
    let symbols = index.symbols()?;
    let graph = FileGraph::new(&symbols, history, learned);
    let vocabulary = Vocabulary::build(&symbols, &graph.paths);
    let words = stems(task);
    let scores = vocabulary.scores(task);
    let top = scores.values().copied().fold(0.0, f64::max);
    let seeds: BTreeMap<usize, f64> = scores
        .into_iter()
        .filter(|(_, score)| *score > 0.0)
        .map(|(node, score)| (node, score / top))
        .collect();
    let mut matches: BTreeMap<usize, Vec<&LocatedSymbol>> = BTreeMap::new();
    for located in &symbols {
        if located.symbol.kind != SymbolKind::Reference
            && let Some(node) = graph.position(&located.path)
            && Vocabulary::relates(&located.symbol.name, &words)
        {
            matches.entry(node).or_default().push(located);
        }
    }
    if seeds.is_empty() {
        return Ok(format!(
            "Nothing in the index matches {}. Try other words, or `find_symbol` with a name.\n",
            keywords.join(", ")
        ));
    }
    let mut ranks = lift(&graph.rank(&seeds), &graph.rank(&BTreeMap::new()));
    penalise_tests(&graph.paths, &mut ranks, &keywords);
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

/// Facts found by framework recipes, grouped by kind, optionally only the kinds whose name
/// or title contains `kind`.
///
/// # Errors
///
/// Returns an error when the index cannot be read.
pub fn facts(index: &Index, project: &Path, kind: Option<&str>) -> Result<String, StoreError> {
    let titles: BTreeMap<String, String> = recipes::titles(project).into_iter().collect();
    let wanted = kind
        .map(str::to_lowercase)
        .filter(|kind| !kind.trim().is_empty());
    let mut groups: BTreeMap<String, Vec<LocatedSymbol>> = BTreeMap::new();
    for fact in index.facts()? {
        let title = titles
            .get(&fact.symbol.role)
            .cloned()
            .unwrap_or_else(|| fact.symbol.role.clone());
        let heading = format!(
            "{title} ({})",
            fact.symbol.role.split('/').next().unwrap_or_default()
        );
        if wanted.as_ref().is_none_or(|wanted| {
            heading.to_lowercase().contains(wanted.as_str())
                || fact.symbol.role.to_lowercase().contains(wanted.as_str())
        }) {
            groups.entry(heading).or_default().push(fact);
        }
    }
    if groups.is_empty() {
        return Ok(match wanted {
            Some(kind) => format!(
                "No framework fact matches `{kind}`. Call `facts` without a kind to see every kind.\n"
            ),
            None => {
                "No framework recipe applies to this project, so there are no framework facts.\n"
                    .to_owned()
            }
        });
    }
    let mut text = String::new();
    for (heading, facts) in &groups {
        let _ = writeln!(text, "## {heading}, {} found", facts.len());
        for fact in facts.iter().take(MAX_FACTS_PER_KIND) {
            let _ = writeln!(
                text,
                "- {}:{} {}",
                fact.path, fact.symbol.line, fact.symbol.name
            );
        }
        if facts.len() > MAX_FACTS_PER_KIND {
            let _ = writeln!(
                text,
                "- and {} more; pass a more specific kind or use `find_symbol`",
                facts.len() - MAX_FACTS_PER_KIND
            );
        }
        text.push('\n');
    }
    Ok(text)
}

/// Divides the score of test and documentation files by the test penalty, unless the task is
/// about tests or documentation.
#[allow(
    clippy::cast_precision_loss,
    reason = "the penalty is a small constant"
)]
pub(crate) fn penalise_tests(paths: &[String], scores: &mut [f64], keywords: &[String]) {
    let about = |prefixes: &[&str]| {
        keywords
            .iter()
            .any(|keyword| prefixes.iter().any(|prefix| keyword.starts_with(prefix)))
    };
    let about_tests = about(&["test", "spec"]);
    let about_docs = about(&["doc", "readme", "changelog", "guide"]);
    for (path, score) in paths.iter().zip(scores.iter_mut()) {
        if (!about_tests && is_test_path(path)) || (!about_docs && is_doc_path(path)) {
            *score /= TEST_PENALTY as f64;
        }
    }
}

fn is_doc_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    [".md", ".rst", ".txt", ".adoc"]
        .iter()
        .any(|extension| lower.ends_with(extension))
        || lower.starts_with("docs/")
        || lower.contains("/docs/")
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
        SymbolKind::Fact => format!("{} ({})", symbol.name, symbol.role),
        SymbolKind::Doc => format!("{}: {}", symbol.role, symbol.name),
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
