//! Extraction of definitions and references with the tags query of each language.

use std::collections::BTreeSet;

use streaming_iterator::StreamingIterator;
use tree_sitter::{Node, Parser, QueryCursor};
use tree_sitter_language_pack::{QueryKind, get_query};

use crate::grammar::{self, GrammarError};

const MAX_SIGNATURE_CHARS: usize = 160;

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
    let query = get_query(language, QueryKind::Tags)
        .ok()
        .flatten()
        .ok_or_else(|| GrammarError::NoTagsQuery {
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
    let signature = if kind == TagKind::Definition {
        signature(node, source)
    } else {
        String::new()
    };
    Tag {
        line: line_number(node.start_position().row),
        kind,
        name: name.to_owned(),
        role: role.to_owned(),
        end_line: line_number(node.end_position().row),
        signature,
    }
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
