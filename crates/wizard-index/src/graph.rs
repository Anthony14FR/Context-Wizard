//! The file graph of a project: who refers to whom, and what changes together.
//!
//! Each file is a node. A reference to a name adds an edge from the referring file to every
//! file defining that name, weighted by one over the number of definers. Files that changed
//! together in recent commits are linked both ways, weighted by how often that happened.

use std::collections::{BTreeMap, BTreeSet};

use wizard_git::History;
use wizard_graph::{Graph, GraphBuilder, PageRankOptions, pagerank};
use wizard_store::{LocatedSymbol, SymbolKind};

const CO_CHANGE_WEIGHT: f64 = 0.5;

/// Files of the project with the graph linking them.
#[derive(Debug, Clone)]
pub struct FileGraph {
    /// Paths of the nodes, sorted; a node's index is its position.
    pub paths: Vec<String>,
    /// Edges between files.
    pub graph: Graph,
    /// Referring files of each file, by node index.
    pub referrers: Vec<BTreeSet<usize>>,
    /// Files each file refers to, by node index.
    pub dependencies: Vec<BTreeSet<usize>>,
}

impl FileGraph {
    /// Builds the graph from the stored symbols and, when available, the git history.
    #[must_use]
    pub fn new(symbols: &[LocatedSymbol], history: Option<&History>) -> Self {
        let paths: Vec<String> = symbols
            .iter()
            .map(|located| located.path.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let index: BTreeMap<&str, usize> = paths
            .iter()
            .enumerate()
            .map(|(position, path)| (path.as_str(), position))
            .collect();
        let mut definers: BTreeMap<&str, BTreeSet<usize>> = BTreeMap::new();
        for located in symbols {
            if located.symbol.kind == SymbolKind::Definition {
                definers
                    .entry(located.symbol.name.as_str())
                    .or_default()
                    .insert(index[located.path.as_str()]);
            }
        }
        let mut builder = GraphBuilder::new(paths.len());
        let mut referrers = vec![BTreeSet::new(); paths.len()];
        let mut dependencies = vec![BTreeSet::new(); paths.len()];
        for located in symbols {
            if located.symbol.kind != SymbolKind::Reference {
                continue;
            }
            let from = index[located.path.as_str()];
            let Some(targets) = definers.get(located.symbol.name.as_str()) else {
                continue;
            };
            let weight = 1.0 / count(targets.len());
            for &to in targets.iter().filter(|to| **to != from) {
                builder.add_edge(from, to, weight);
                referrers[to].insert(from);
                dependencies[from].insert(to);
            }
        }
        if let Some(history) = history {
            for ((left, right), times) in &history.co_change {
                if let (Some(&left), Some(&right)) =
                    (index.get(left.as_str()), index.get(right.as_str()))
                {
                    let weight = CO_CHANGE_WEIGHT * f64::from(*times);
                    builder.add_edge(left, right, weight);
                    builder.add_edge(right, left, weight);
                }
            }
        }
        Self {
            paths,
            graph: builder.build(),
            referrers,
            dependencies,
        }
    }

    /// Node index of `path`.
    #[must_use]
    pub fn position(&self, path: &str) -> Option<usize> {
        self.paths
            .binary_search_by(|known| known.as_str().cmp(path))
            .ok()
    }

    /// Ranks files, restarting on `seeds` with their weights, or uniformly without seeds.
    #[must_use]
    pub fn rank(&self, seeds: &BTreeMap<usize, f64>) -> Vec<f64> {
        let personalisation: Vec<f64> = (0..self.paths.len())
            .map(|node| seeds.get(&node).copied().unwrap_or(0.0))
            .collect();
        pagerank(
            &self.graph,
            Some(&personalisation),
            &PageRankOptions::default(),
        )
        .ranks
    }
}

#[allow(
    clippy::cast_precision_loss,
    reason = "definer counts stay far below 2^52"
)]
fn count(value: usize) -> f64 {
    value as f64
}
