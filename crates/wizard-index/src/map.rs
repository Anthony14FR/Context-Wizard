//! The L1 map: definitions with their signatures and line numbers, grouped by file and ranked
//! by how many other files refer to them, within a token budget.
//!
//! A definition scores one point per distinct other file that references its name, divided by
//! the number of files that define that name, so generic names such as `new` or `fmt` do not
//! dominate. A file scores the sum of its definitions. Test files rank after production code:
//! their definitions, often mocks of real names, do not count as definers, and their score is
//! divided by ten. Files are listed by decreasing score, each with its most
//! used definitions in line order, until the budget is spent. Rendering stops at the first
//! line that does not fit, so a lower-ranked file is never shown in place of a higher one.

use std::collections::{BTreeMap, BTreeSet};

use wizard_budget::{Content, Estimator};
use wizard_store::{Index, LocatedSymbol, StoreError, SymbolKind, SymbolRecord};

/// Default budget of the map, in estimated tokens.
pub const L1_BUDGET: u64 = 4_000;

const MAX_DEFINITIONS_PER_FILE: usize = 30;
const POINT: usize = 1_000;
pub(crate) const TEST_PENALTY: usize = 10;
const HEADER: &str = "# Repository map (Context Wizard)\nDefinitions ranked by how many other files use them, with line numbers.\n";

struct RankedFile<'a> {
    path: &'a str,
    score: usize,
    definitions: Vec<(&'a SymbolRecord, usize)>,
}

/// Renders the map of the indexed project within `budget` estimated tokens.
///
/// # Errors
///
/// Returns an error when the index cannot be read.
pub fn map(index: &Index, budget: u64) -> Result<String, StoreError> {
    map_with(index, budget, &Estimator::default())
}

/// Renders the map like [`map`], estimating tokens with `estimator`.
///
/// # Errors
///
/// Returns an error when the index cannot be read.
pub fn map_with(index: &Index, budget: u64, estimator: &Estimator) -> Result<String, StoreError> {
    let symbols = index.symbols()?;
    Ok(render(&rank(&symbols), budget, estimator))
}

fn rank(symbols: &[LocatedSymbol]) -> Vec<RankedFile<'_>> {
    let mut referencing: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for located in symbols {
        if located.symbol.kind == SymbolKind::Reference {
            referencing
                .entry(located.symbol.name.as_str())
                .or_default()
                .insert(located.path.as_str());
        }
    }
    let mut defining: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for located in symbols {
        if located.symbol.kind == SymbolKind::Definition && !is_test_path(&located.path) {
            defining
                .entry(located.symbol.name.as_str())
                .or_default()
                .insert(located.path.as_str());
        }
    }
    let mut files: BTreeMap<&str, Vec<(&SymbolRecord, usize)>> = BTreeMap::new();
    for located in symbols {
        if located.symbol.kind == SymbolKind::Definition {
            let name = located.symbol.name.as_str();
            let users = referencing.get(name).map_or(0, |paths| {
                paths.iter().filter(|path| **path != located.path).count()
            });
            let definers = defining.get(name).map_or(1, BTreeSet::len).max(1);
            let score = users * POINT / definers;
            files
                .entry(located.path.as_str())
                .or_default()
                .push((&located.symbol, score));
        }
    }
    let mut ranked: Vec<RankedFile<'_>> = files
        .into_iter()
        .map(|(path, mut definitions)| {
            let total: usize = definitions.iter().map(|(_, score)| score).sum();
            let score = if is_test_path(path) {
                total / TEST_PENALTY
            } else {
                total
            };
            definitions
                .sort_by(|left, right| right.1.cmp(&left.1).then(left.0.line.cmp(&right.0.line)));
            definitions.truncate(MAX_DEFINITIONS_PER_FILE);
            definitions.sort_by_key(|(symbol, _)| symbol.line);
            RankedFile {
                path,
                score,
                definitions,
            }
        })
        .collect();
    ranked.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then(right.definitions.len().cmp(&left.definitions.len()))
            .then(left.path.cmp(right.path))
    });
    ranked
}

fn render(files: &[RankedFile<'_>], budget: u64, estimator: &Estimator) -> String {
    let mut text = HEADER.to_owned();
    if estimator.estimate(&text, Content::Code) > budget {
        return String::new();
    }
    for file in files {
        let mut block = format!("\n{}\n", file.path);
        for (symbol, _) in &file.definitions {
            let line = format!("{:>5} {}\n", symbol.line, describe(symbol));
            let candidate = format!("{text}{block}{line}");
            if estimator.estimate(&candidate, Content::Code) > budget {
                if block.lines().count() > 2 {
                    text.push_str(&block);
                }
                return text;
            }
            block.push_str(&line);
        }
        text.push_str(&block);
    }
    text
}

pub(crate) fn is_test_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let name = lower.rsplit('/').next().unwrap_or(&lower);
    let in_test_folder = lower.split('/').rev().skip(1).any(|segment| {
        matches!(
            segment,
            "test" | "tests" | "__tests__" | "__mocks__" | "spec" | "specs" | "e2e" | "fixtures"
        )
    });
    in_test_folder
        || name.contains(".test.")
        || name.contains(".spec.")
        || name.ends_with("_test.go")
        || name.ends_with("_test.py")
        || name.starts_with("test_")
        || name.ends_with("test.java")
        || name.ends_with("tests.ps1")
}

fn describe(symbol: &SymbolRecord) -> String {
    if symbol.signature.is_empty() {
        format!("{} {}", symbol.role, symbol.name)
    } else {
        symbol.signature.clone()
    }
}

#[cfg(test)]
mod tests {
    use wizard_store::{LocatedSymbol, SymbolKind, SymbolRecord};

    use super::{Estimator, rank, render};

    fn symbol(path: &str, kind: SymbolKind, name: &str, line: u32) -> LocatedSymbol {
        LocatedSymbol {
            path: path.to_owned(),
            symbol: SymbolRecord {
                line,
                kind,
                name: name.to_owned(),
                role: "function".to_owned(),
                end_line: line,
                signature: if kind == SymbolKind::Definition {
                    format!("fn {name}()")
                } else {
                    String::new()
                },
                count: 1,
            },
        }
    }

    fn sample() -> Vec<LocatedSymbol> {
        vec![
            symbol("src/core.rs", SymbolKind::Definition, "parse", 3),
            symbol("src/core.rs", SymbolKind::Definition, "helper", 10),
            symbol("src/core.rs", SymbolKind::Reference, "helper", 4),
            symbol("src/a.rs", SymbolKind::Definition, "run", 1),
            symbol("src/a.rs", SymbolKind::Reference, "parse", 2),
            symbol("src/b.rs", SymbolKind::Reference, "parse", 7),
            symbol("src/b.rs", SymbolKind::Reference, "run", 8),
        ]
    }

    #[test]
    fn ranks_files_by_cross_file_references() {
        let symbols = sample();
        let ranked = rank(&symbols);
        let order: Vec<(&str, usize)> = ranked.iter().map(|file| (file.path, file.score)).collect();
        assert_eq!(order, [("src/core.rs", 2_000), ("src/a.rs", 1_000)]);
    }

    #[test]
    fn renders_signatures_in_line_order() {
        let symbols = sample();
        let text = render(&rank(&symbols), 4_000, &Estimator::default());
        assert!(
            text.contains("\nsrc/core.rs\n    3 fn parse()\n   10 fn helper()\n"),
            "{text}"
        );
        assert!(text.find("src/core.rs") < text.find("src/a.rs"));
    }

    #[test]
    fn ranks_test_files_after_production_code() {
        let symbols = vec![
            symbol("src/theme.ts", SymbolKind::Definition, "useTheme", 1),
            symbol("src/Button.test.tsx", SymbolKind::Definition, "useTheme", 3),
            symbol(
                "src/Button.test.tsx",
                SymbolKind::Definition,
                "useLingui",
                4,
            ),
            symbol("src/i18n.ts", SymbolKind::Definition, "useLingui", 1),
            symbol("src/a.tsx", SymbolKind::Reference, "useTheme", 1),
            symbol("src/b.tsx", SymbolKind::Reference, "useTheme", 1),
            symbol("src/a.tsx", SymbolKind::Reference, "useLingui", 2),
        ];
        let ranked = rank(&symbols);
        let order: Vec<&str> = ranked.iter().map(|file| file.path).collect();
        assert_eq!(
            order,
            ["src/theme.ts", "src/i18n.ts", "src/Button.test.tsx"]
        );
        assert!(super::is_test_path("pkg/server_test.go"));
        assert!(super::is_test_path("tests/test_app.py"));
        assert!(!super::is_test_path("src/testing/harness.ts"));
    }

    #[test]
    fn stays_within_the_budget() {
        let mut symbols = Vec::new();
        for file in 0..200 {
            for line in 0..20 {
                symbols.push(symbol(
                    &format!("src/module_{file:03}.rs"),
                    SymbolKind::Definition,
                    &format!("function_number_{line}"),
                    line + 1,
                ));
            }
        }
        let text = render(&rank(&symbols), 500, &Estimator::default());
        let estimate = Estimator::default().estimate(&text, wizard_budget::Content::Code);
        assert!(estimate <= 500, "{estimate}");
        assert!(text.contains("src/module_000.rs"));
    }
}
