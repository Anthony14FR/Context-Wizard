//! The file graph and its global ranks, cached in the index for the hooks.
//!
//! Building the graph reads every symbol, which is too slow for a hook on a large project, so
//! the indexer stores it once per change of the code and hooks read it back in one row.
//!
//! Relevance to a task is the lift of a file: its rank when the random surfer restarts on the
//! task's files, divided by its rank when the surfer restarts anywhere. Hub files that every
//! ranking favours, such as shared helpers, therefore stop crowding out the files of the task.

use std::collections::{BTreeMap, BTreeSet};

use wizard_graph::{Graph, PageRankOptions, pagerank};
use wizard_store::{Index, LocatedSymbol, StoreError, SymbolKind};

use crate::graph::FileGraph;
use crate::learned::{Learned, LearnedPair};

/// Name of the cached structure in the index.
pub const STRUCTURE_CACHE: &str = "structure-v1";

const CO_ACCESS_WEIGHT: f64 = 0.5;
const RELEVANCE_TOLERANCE: f64 = 1e-6;

/// The file graph of a project with the global rank of each file.
#[derive(Debug, Clone, PartialEq)]
pub struct Structure {
    /// Paths of the nodes, sorted; a node's index is its position.
    pub paths: Vec<String>,
    /// Edges between files, from references and definitions.
    pub graph: Graph,
    /// Rank of each file with a uniform restart.
    pub global: Vec<f64>,
    /// Distinct names of the definitions, headings and keys, sorted.
    pub names: Vec<String>,
}

impl Structure {
    /// Builds the structure from the symbols of the index.
    #[must_use]
    pub fn build(symbols: &[LocatedSymbol]) -> Self {
        let files = FileGraph::new(symbols, None, &Learned::default());
        let global = pagerank(&files.graph, None, &PageRankOptions::default()).ranks;
        let names: BTreeSet<&str> = symbols
            .iter()
            .filter(|located| located.symbol.kind != SymbolKind::Reference)
            .map(|located| located.symbol.name.as_str())
            .collect();
        Self {
            paths: files.paths,
            graph: files.graph,
            global,
            names: names.into_iter().map(str::to_owned).collect(),
        }
    }

    /// Reads the cached structure of `index`, building and caching it when missing.
    ///
    /// # Errors
    ///
    /// Returns an error when the index cannot be read.
    pub fn load(index: &mut Index) -> Result<Self, StoreError> {
        if let Some(structure) = index
            .cached(STRUCTURE_CACHE)?
            .as_deref()
            .and_then(Self::from_bytes)
        {
            return Ok(structure);
        }
        let structure = Self::build(&index.symbols()?);
        let _ = index.set_cached(STRUCTURE_CACHE, &structure.to_bytes());
        Ok(structure)
    }

    /// Node index of `path`.
    #[must_use]
    pub fn position(&self, path: &str) -> Option<usize> {
        self.paths
            .binary_search_by(|known| known.as_str().cmp(path))
            .ok()
    }

    /// Lift of every file for a task seeded on `seeds`, with learned co-access pairs added to
    /// the graph.
    #[must_use]
    pub fn relevance(&self, seeds: &BTreeMap<usize, f64>, co_access: &[LearnedPair]) -> Vec<f64> {
        let extended;
        let graph = if co_access.is_empty() {
            &self.graph
        } else {
            let mut builder = self.graph.to_builder();
            for pair in co_access {
                if let (Some(first), Some(second)) =
                    (self.position(&pair.first), self.position(&pair.second))
                {
                    builder.add_edge(first, second, CO_ACCESS_WEIGHT * pair.weight);
                    builder.add_edge(second, first, CO_ACCESS_WEIGHT * pair.weight);
                }
            }
            extended = builder.build();
            &extended
        };
        let personalisation: Vec<f64> = (0..self.paths.len())
            .map(|node| seeds.get(&node).copied().unwrap_or(0.0))
            .collect();
        let options = PageRankOptions {
            tolerance: RELEVANCE_TOLERANCE,
            ..PageRankOptions::default()
        };
        let ranks = pagerank(graph, Some(&personalisation), &options).ranks;
        lift(&ranks, &self.global)
    }

    /// Encodes the structure for the index cache.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        for list in [&self.paths, &self.names] {
            bytes.extend_from_slice(&(list.len() as u64).to_le_bytes());
            for text in list {
                bytes.extend_from_slice(&(text.len() as u64).to_le_bytes());
                bytes.extend_from_slice(text.as_bytes());
            }
        }
        for rank in &self.global {
            bytes.extend_from_slice(&rank.to_le_bytes());
        }
        bytes.extend_from_slice(&self.graph.to_bytes());
        bytes
    }

    /// Decodes a structure written by [`Structure::to_bytes`].
    #[must_use]
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let (paths, rest) = split_strings(bytes)?;
        let (names, mut rest) = split_strings(rest)?;
        let count = paths.len();
        let mut global = Vec::with_capacity(count);
        for _ in 0..count {
            let (rank, after) = rest.split_first_chunk::<8>()?;
            global.push(f64::from_le_bytes(*rank));
            rest = after;
        }
        let graph = Graph::from_bytes(rest)?;
        (graph.nodes() == count && paths.is_sorted()).then_some(Self {
            paths,
            graph,
            global,
            names,
        })
    }
}

/// Rank of each node relative to its global rank, smoothed so tiny global ranks do not explode.
#[must_use]
#[allow(
    clippy::cast_precision_loss,
    reason = "node counts stay far below 2^52"
)]
pub fn lift(ranks: &[f64], global: &[f64]) -> Vec<f64> {
    let floor = 1.0 / ranks.len().max(1) as f64;
    ranks
        .iter()
        .zip(global)
        .map(|(rank, global)| rank / (global + floor))
        .collect()
}

fn split_strings(bytes: &[u8]) -> Option<(Vec<String>, &[u8])> {
    let (count, mut rest) = split_u64(bytes)?;
    let count = usize::try_from(count).ok()?;
    let mut strings = Vec::with_capacity(count.min(1 << 20));
    for _ in 0..count {
        let (length, after) = split_u64(rest)?;
        let length = usize::try_from(length).ok()?;
        if after.len() < length {
            return None;
        }
        let (text, after) = after.split_at(length);
        strings.push(String::from_utf8(text.to_vec()).ok()?);
        rest = after;
    }
    Some((strings, rest))
}

fn split_u64(bytes: &[u8]) -> Option<(u64, &[u8])> {
    let (head, rest) = bytes.split_first_chunk::<8>()?;
    Some((u64::from_le_bytes(*head), rest))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::collections::BTreeMap;

    use wizard_store::{LocatedSymbol, SymbolKind, SymbolRecord};

    use super::{Structure, lift};
    use crate::learned::LearnedPair;

    fn symbol(path: &str, name: &str, kind: SymbolKind) -> LocatedSymbol {
        LocatedSymbol {
            path: path.to_owned(),
            symbol: SymbolRecord {
                line: 1,
                kind,
                name: name.to_owned(),
                role: String::new(),
                end_line: 1,
                signature: String::new(),
                count: 1,
            },
        }
    }

    fn hub_project() -> Vec<LocatedSymbol> {
        let mut symbols = vec![
            symbol("src/helpers.rs", "helper", SymbolKind::Definition),
            symbol("src/invoice.rs", "invoice", SymbolKind::Definition),
            symbol("src/tax.rs", "tax", SymbolKind::Definition),
            symbol("src/invoice.rs", "tax", SymbolKind::Reference),
        ];
        for user in ["a", "b", "c", "d", "e", "f"] {
            symbols.push(symbol(
                &format!("src/{user}.rs"),
                user,
                SymbolKind::Definition,
            ));
            symbols.push(symbol(
                &format!("src/{user}.rs"),
                "helper",
                SymbolKind::Reference,
            ));
        }
        symbols.push(symbol("src/invoice.rs", "helper", SymbolKind::Reference));
        symbols
    }

    #[test]
    fn round_trips_through_bytes() {
        let structure = Structure::build(&hub_project());
        let bytes = structure.to_bytes();
        assert_eq!(Structure::from_bytes(&bytes), Some(structure.clone()));
        assert_eq!(Structure::from_bytes(&bytes[..10]), None);
    }

    #[test]
    fn lift_keeps_hubs_from_crowding_out_the_task() {
        let structure = Structure::build(&hub_project());
        let invoice = structure.position("src/invoice.rs").unwrap();
        let tax = structure.position("src/tax.rs").unwrap();
        let helpers = structure.position("src/helpers.rs").unwrap();
        let seeds = BTreeMap::from([(invoice, 1.0)]);
        let relevance = structure.relevance(&seeds, &[]);
        assert!(relevance[tax] > relevance[helpers], "{relevance:?}");
        let pairs = [LearnedPair {
            first: "src/a.rs".to_owned(),
            second: "src/invoice.rs".to_owned(),
            weight: 5.0,
        }];
        let a = structure.position("src/a.rs").unwrap();
        let with_learning = structure.relevance(&seeds, &pairs);
        assert!(with_learning[a] > relevance[a]);
        assert_eq!(lift(&[0.5], &[0.0]), [0.5]);
    }
}
