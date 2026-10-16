//! Answers to a Grep for a known symbol, added next to the search results.
//!
//! When the pattern of a Grep call names a symbol the index knows, the hook adds its
//! definitions and the number of files using it. The search still runs: this only saves the
//! follow-up reads that would locate the definition.

use std::fmt::Write as _;
use std::path::Path;

use wizard_store::{Index, SymbolKind};

const MAX_DEFINITIONS: usize = 5;
const KEYWORDS: [&str; 14] = [
    "fn",
    "def",
    "class",
    "function",
    "func",
    "struct",
    "interface",
    "type",
    "enum",
    "trait",
    "impl",
    "const",
    "let",
    "var",
];

/// Describes the definitions of the symbol searched by `pattern`, when the index knows it.
pub(crate) fn known_symbol(project: &Path, pattern: &str) -> Option<String> {
    let name = symbol_name(pattern)?;
    let index = Index::open_project(project).ok()?;
    let symbols = index.symbols_named(&name).ok()?;
    let definitions: Vec<_> = symbols
        .iter()
        .filter(|located| located.symbol.kind == SymbolKind::Definition)
        .collect();
    if definitions.is_empty() {
        return None;
    }
    let mut users: Vec<&str> = symbols
        .iter()
        .filter(|located| located.symbol.kind == SymbolKind::Reference)
        .map(|located| located.path.as_str())
        .collect();
    users.sort_unstable();
    users.dedup();
    let mut text = format!("Context Wizard index: `{name}` is defined at:\n");
    for located in definitions.iter().take(MAX_DEFINITIONS) {
        let _ = writeln!(
            text,
            "- {}:{} {}",
            located.path, located.symbol.line, located.symbol.signature
        );
    }
    let _ = writeln!(
        text,
        "It is used in {} files. The refs tool of Context Wizard lists every use with line numbers.",
        users.len()
    );
    Some(text)
}

fn symbol_name(pattern: &str) -> Option<String> {
    let cleaned = pattern
        .replace(r"\b", " ")
        .replace(r"\(", " ")
        .replace(['^', '$', '(', ')'], " ");
    let tokens: Vec<&str> = cleaned.split_whitespace().collect();
    let candidate = match tokens.as_slice() {
        [name] => *name,
        [keyword, name] if KEYWORDS.contains(keyword) => *name,
        _ => return None,
    };
    let mut characters = candidate.chars();
    let valid = characters
        .next()
        .is_some_and(|first| first.is_alphabetic() || first == '_')
        && characters.all(|character| character.is_alphanumeric() || character == '_');
    (valid && candidate.chars().count() >= 3).then(|| candidate.to_owned())
}

#[cfg(test)]
mod tests {
    use super::symbol_name;

    #[test]
    fn recognises_symbol_searches() {
        for (pattern, name) in [
            ("compute_invoice", "compute_invoice"),
            (r"\bSessionStore\b", "SessionStore"),
            ("fn build_options", "build_options"),
            ("def load_token", "load_token"),
            ("class FeedTuner", "FeedTuner"),
            (r"useSession\(", "useSession"),
        ] {
            assert_eq!(symbol_name(pattern).as_deref(), Some(name), "{pattern}");
        }
    }

    #[test]
    fn ignores_text_searches() {
        for pattern in [
            "TODO: fix",
            "a.*b",
            "error|warning",
            "is",
            "fn x",
            "foo bar baz",
        ] {
            assert_eq!(symbol_name(pattern), None, "{pattern}");
        }
    }
}
