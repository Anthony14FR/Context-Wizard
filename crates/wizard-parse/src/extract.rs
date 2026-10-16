//! Extraction of definitions and references with the tags query of each language.

use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, LazyLock, Mutex};

use streaming_iterator::StreamingIterator;
use tree_sitter::{Language, Node, Parser, Query, QueryCursor};
use tree_sitter_language_pack::{QueryKind, get_query, get_tags_query};

use crate::grammar::{self, GrammarError};

const MAX_DOC_COMMENTS: usize = 8;
const MAX_DOC_CHARS: usize = 200;
const MAX_SIGNATURE_CHARS: usize = 160;

static EXTENDED_QUERIES: LazyLock<Mutex<HashMap<String, Option<Arc<Query>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Whether a tag defines a name or refers to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TagKind {
    /// The name is defined here.
    Definition,
    /// The name is used here.
    Reference,
}

/// A named definition or reference found in a file.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Tag {
    /// First line of the tagged node, numbered from 1.
    pub line: u32,
    /// Definition or reference.
    pub kind: TagKind,
    /// The defined or referenced name.
    pub name: String,
    /// Role given by the tags query, such as `function`, `class` or `call`.
    pub role: String,
    /// Last line of the tagged node, numbered from 1.
    pub end_line: u32,
    /// First line of a definition, trimmed and shortened; empty for references.
    pub signature: String,
    /// First sentence of the comment documenting a definition; empty when there is none.
    pub doc: String,
}

/// Parses `source` as `language` and returns its definitions and references, sorted by line.
///
/// A name tagged twice on the same line with different roles, such as a method that is also
/// matched as a function, is kept once.
///
/// # Errors
///
/// Returns an error when the grammar is unavailable, the language has no tags query, or the
/// source cannot be parsed.
pub fn extract(language: &str, source: &str) -> Result<Vec<Tag>, GrammarError> {
    let grammar = grammar::language(language)?;
    let query = tags_query(language, &grammar).ok_or_else(|| GrammarError::NoTagsQuery {
        language: language.to_owned(),
    })?;
    let parse_error = || GrammarError::Parse {
        language: language.to_owned(),
    };
    let mut parser = Parser::new();
    parser.set_language(&grammar).map_err(|_| parse_error())?;
    let tree = parser.parse(source, None).ok_or_else(parse_error)?;
    let names = query.capture_names();
    let mut tags = BTreeSet::new();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), source.as_bytes());
    while let Some(found) = matches.next() {
        let mut name = None;
        let mut tagged = None;
        for capture in found.captures() {
            let capture_name = names
                .get(usize::try_from(capture.index).unwrap_or(usize::MAX))
                .copied()
                .unwrap_or_default();
            if capture_name == "name" {
                name = Some(capture.node);
            } else if let Some((kind, role)) = classify(capture_name) {
                tagged = Some((kind, role, capture.node));
            }
        }
        if let (Some(name), Some((kind, role, node))) = (name, tagged)
            && let Ok(text) = name.utf8_text(source.as_bytes())
        {
            tags.insert(tag(kind, role, text, node, source));
        }
    }
    let mut tags: Vec<Tag> = tags.into_iter().collect();
    tags.dedup_by(|later, earlier| {
        let same =
            later.line == earlier.line && later.kind == earlier.kind && later.name == earlier.name;
        if same && earlier.role == "function" && later.role != "function" {
            std::mem::swap(later, earlier);
        }
        same
    });
    Ok(tags)
}

/// Returns the tags query of `language`.
///
/// The TypeScript tags queries only cover TypeScript-specific constructs and are meant to extend
/// the JavaScript one, so both are combined for `typescript` and `tsx`, falling back to the
/// TypeScript query alone when the combination does not compile for the grammar.
fn tags_query(language: &str, grammar: &Language) -> Option<Arc<Query>> {
    if !matches!(language, "typescript" | "tsx") {
        return get_query(language, QueryKind::Tags).ok().flatten();
    }
    if let Ok(cache) = EXTENDED_QUERIES.lock()
        && let Some(query) = cache.get(language)
    {
        return query.clone();
    }
    let own = get_tags_query(language).unwrap_or_default();
    let combined = format!(
        "{}
{own}",
        get_tags_query("javascript").unwrap_or_default()
    );
    let query = Query::new(grammar, &combined)
        .or_else(|_| Query::new(grammar, own))
        .ok()
        .map(Arc::new);
    if let Ok(mut cache) = EXTENDED_QUERIES.lock() {
        cache.insert(language.to_owned(), query.clone());
    }
    query
}

fn classify(capture_name: &str) -> Option<(TagKind, &str)> {
    if let Some(role) = capture_name.strip_prefix("definition.") {
        Some((TagKind::Definition, role))
    } else {
        capture_name
            .strip_prefix("reference.")
            .map(|role| (TagKind::Reference, role))
    }
}

fn tag(kind: TagKind, role: &str, name: &str, node: Node<'_>, source: &str) -> Tag {
    let (signature, doc) = if kind == TagKind::Definition {
        (signature(node, source), documentation(node, source))
    } else {
        (String::new(), String::new())
    };
    Tag {
        line: line_number(node.start_position().row),
        kind,
        name: name.to_owned(),
        role: role.to_owned(),
        end_line: line_number(node.end_position().row),
        signature,
        doc,
    }
}

/// The first sentence of the comment just above `node`, or of a Python docstring, cleaned of
/// comment markers. Comments above an `export` or a decorator count for what they wrap.
fn documentation(node: Node<'_>, source: &str) -> String {
    let mut holder = node;
    while let Some(parent) = holder.parent() {
        let kind = parent.kind();
        if kind.contains("export") || kind.contains("decorated") || kind == "attribute_item" {
            holder = parent;
        } else {
            break;
        }
    }
    let mut comments = Vec::new();
    let mut previous = holder.prev_sibling();
    while let Some(sibling) = previous {
        if !sibling.kind().contains("comment") || comments.len() == MAX_DOC_COMMENTS {
            break;
        }
        comments.push(source.get(sibling.byte_range()).unwrap_or_default());
        previous = sibling.prev_sibling();
    }
    comments.reverse();
    let mut text = comments.join("\n");
    if text.is_empty()
        && let Some(body) = node.child_by_field_name("body")
        && let Some(first) = body.named_child(0)
        && let Some(string) = if first.kind() == "expression_statement" {
            first.named_child(0)
        } else {
            Some(first)
        }
        && string.kind() == "string"
    {
        source
            .get(string.byte_range())
            .unwrap_or_default()
            .clone_into(&mut text);
    }
    first_sentence(&text)
}

fn first_sentence(comment: &str) -> String {
    let words: Vec<&str> = comment
        .lines()
        .map(|line| {
            line.trim()
                .trim_start_matches(['/', '*', '#', '!', '-', '"', '\''])
                .trim_end_matches(['/', '*', '"', '\''])
                .trim()
        })
        .filter(|line| !line.starts_with('@') && !line.is_empty())
        .flat_map(str::split_whitespace)
        .collect();
    let text = words.join(" ");
    let lower = text.to_lowercase();
    if lower.contains("license") || lower.contains("copyright") {
        return String::new();
    }
    let end = text
        .char_indices()
        .find(|(position, character)| *character == '.' && text[position + 1..].starts_with(' '))
        .map_or(text.len(), |(position, _)| position + 1);
    text[..end].chars().take(MAX_DOC_CHARS).collect()
}

fn signature(node: Node<'_>, source: &str) -> String {
    let text = source.get(node.byte_range()).unwrap_or_default();
    let first_line = text.lines().next().unwrap_or_default().trim();
    let mut signature: String = first_line.chars().take(MAX_SIGNATURE_CHARS).collect();
    if first_line.chars().count() > MAX_SIGNATURE_CHARS {
        signature.push('…');
    }
    signature
}

fn line_number(row: usize) -> u32 {
    u32::try_from(row.saturating_add(1)).unwrap_or(u32::MAX)
}
