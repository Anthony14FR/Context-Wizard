//! Weighted directed graph in compressed sparse row form and personalised `PageRank`.
//!
//! The graph stores outgoing edges of each node contiguously, so memory and each `PageRank`
//! iteration are linear in the number of edges. `PageRank` runs as a power iteration with a
//! personalisation vector: the random surfer restarts on nodes in proportion to that vector,
//! which biases the ranking towards the files a task is about. Rank held by nodes without
//! outgoing edges is redistributed the same way. The iteration stops when the L1 change drops
//! below the tolerance or after the iteration cap.

/// Builds a [`Graph`] from individual edges.
#[derive(Debug, Clone, Default)]
pub struct GraphBuilder {
    nodes: usize,
    edges: Vec<(u32, u32, f64)>,
}

impl GraphBuilder {
    /// Starts a graph of `nodes` nodes, numbered from 0.
    #[must_use]
    pub fn new(nodes: usize) -> Self {
        Self {
            nodes,
            edges: Vec::new(),
        }
    }

    /// Adds a directed edge. Edges with a node out of range, or a weight that is not a positive
    /// finite number, are ignored. Parallel edges add up.
    pub fn add_edge(&mut self, from: usize, to: usize, weight: f64) {
        let (Ok(from), Ok(to)) = (u32::try_from(from), u32::try_from(to)) else {
            return;
        };
        if usize::try_from(from).is_ok_and(|from| from < self.nodes)
            && usize::try_from(to).is_ok_and(|to| to < self.nodes)
            && weight.is_finite()
            && weight > 0.0
        {
            self.edges.push((from, to, weight));
        }
    }

    /// Freezes the edges into compressed sparse row form.
    #[must_use]
    pub fn build(mut self) -> Graph {
        self.edges.sort_unstable_by_key(|edge| (edge.0, edge.1));
        let mut offsets = vec![0; self.nodes + 1];
        let mut targets: Vec<u32> = Vec::with_capacity(self.edges.len());
        let mut weights: Vec<f64> = Vec::with_capacity(self.edges.len());
        let mut previous: Option<(u32, u32)> = None;
        for (from, to, weight) in self.edges {
            if previous == Some((from, to)) {
                if let Some(last) = weights.last_mut() {
                    *last += weight;
                }
                continue;
            }
            previous = Some((from, to));
            targets.push(to);
            weights.push(weight);
            offsets[index(from) + 1] += 1;
        }
        for node in 0..self.nodes {
            offsets[node + 1] += offsets[node];
        }
        let out_weight = (0..self.nodes)
            .map(|node| weights[offsets[node]..offsets[node + 1]].iter().sum())
            .collect();
        Graph {
            offsets,
            targets,
            weights,
            out_weight,
        }
    }
}

/// A weighted directed graph in compressed sparse row form.
#[derive(Debug, Clone, PartialEq)]
pub struct Graph {
    offsets: Vec<usize>,
    targets: Vec<u32>,
    weights: Vec<f64>,
    out_weight: Vec<f64>,
}

impl Graph {
    /// Number of nodes.
    #[must_use]
    pub fn nodes(&self) -> usize {
        self.out_weight.len()
    }

    /// Number of distinct edges.
    #[must_use]
    pub fn edges(&self) -> usize {
        self.targets.len()
    }

    /// Encodes the graph in a compact little-endian form that [`Graph::from_bytes`] reads back.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(16 + self.offsets.len() * 8 + self.targets.len() * 12);
        bytes.extend_from_slice(&(self.nodes() as u64).to_le_bytes());
        bytes.extend_from_slice(&(self.edges() as u64).to_le_bytes());
        for offset in &self.offsets {
            bytes.extend_from_slice(&(*offset as u64).to_le_bytes());
        }
        for target in &self.targets {
            bytes.extend_from_slice(&target.to_le_bytes());
        }
        for weight in &self.weights {
            bytes.extend_from_slice(&weight.to_le_bytes());
        }
        bytes
    }

    /// Decodes a graph written by [`Graph::to_bytes`], or returns `None` when `bytes` is not one.
    #[must_use]
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let mut reader = Reader { bytes };
        let nodes = usize::try_from(reader.u64()?).ok()?;
        let edges = usize::try_from(reader.u64()?).ok()?;
        let offsets = (0..=nodes)
            .map(|_| usize::try_from(reader.u64()?).ok())
            .collect::<Option<Vec<usize>>>()?;
        let targets = (0..edges)
            .map(|_| reader.u32())
            .collect::<Option<Vec<u32>>>()?;
        let weights = (0..edges)
            .map(|_| reader.f64())
            .collect::<Option<Vec<f64>>>()?;
        let valid = reader.bytes.is_empty()
            && offsets.first() == Some(&0)
            && offsets.last() == Some(&edges)
            && offsets.windows(2).all(|pair| pair[0] <= pair[1])
            && targets.iter().all(|target| index(*target) < nodes);
        if !valid {
            return None;
        }
        let out_weight = (0..nodes)
            .map(|node| weights[offsets[node]..offsets[node + 1]].iter().sum())
            .collect();
        Some(Self {
            offsets,
            targets,
            weights,
            out_weight,
        })
    }

    /// Starts a builder holding every edge of this graph, to extend it with more edges.
    #[must_use]
    pub fn to_builder(&self) -> GraphBuilder {
        let mut builder = GraphBuilder::new(self.nodes());
        for node in 0..self.nodes() {
            for (target, weight) in self.neighbours(node) {
                builder.add_edge(node, target, weight);
            }
        }
        builder
    }

    /// Outgoing edges of `node` as `(target, weight)` pairs, sorted by target.
    pub fn neighbours(&self, node: usize) -> impl Iterator<Item = (usize, f64)> + '_ {
        let range = self
            .offsets
            .get(node)
            .copied()
            .zip(self.offsets.get(node + 1).copied())
            .map_or(0..0, |(start, end)| start..end);
        self.targets[range.clone()]
            .iter()
            .zip(&self.weights[range])
            .map(|(target, weight)| (index(*target), *weight))
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
}

impl Reader<'_> {
    fn take<const N: usize>(&mut self) -> Option<[u8; N]> {
        let (head, rest) = self.bytes.split_first_chunk::<N>()?;
        self.bytes = rest;
        Some(*head)
    }

    fn u64(&mut self) -> Option<u64> {
        self.take().map(u64::from_le_bytes)
    }

    fn u32(&mut self) -> Option<u32> {
        self.take().map(u32::from_le_bytes)
    }

    fn f64(&mut self) -> Option<f64> {
        self.take().map(f64::from_le_bytes)
    }
}

/// Settings of a `PageRank` computation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageRankOptions {
    /// Probability of following an edge rather than restarting, between 0 and 1.
    pub damping: f64,
    /// The iteration stops when the L1 change of the ranks drops below this value.
    pub tolerance: f64,
    /// Upper bound on the number of iterations.
    pub max_iterations: usize,
}

impl Default for PageRankOptions {
    fn default() -> Self {
        Self {
            damping: 0.85,
            tolerance: 1e-9,
            max_iterations: 100,
        }
    }
}

/// Ranks of a `PageRank` computation, with the number of iterations it took.
#[derive(Debug, Clone, PartialEq)]
pub struct Ranking {
    /// Rank of each node; non-negative and summing to 1 when the graph has nodes.
    pub ranks: Vec<f64>,
    /// Number of iterations performed.
    pub iterations: usize,
}

/// Computes `PageRank` over `graph`, restarting in proportion to `personalisation`.
///
/// A missing, wrongly sized, negative or all-zero personalisation means a uniform restart.
#[must_use]
pub fn pagerank(
    graph: &Graph,
    personalisation: Option<&[f64]>,
    options: &PageRankOptions,
) -> Ranking {
    let nodes = graph.nodes();
    if nodes == 0 {
        return Ranking {
            ranks: Vec::new(),
            iterations: 0,
        };
    }
    let restart = normalised_restart(nodes, personalisation);
    let damping = options.damping.clamp(0.0, 1.0);
    let mut ranks = restart.clone();
    let mut next = vec![0.0; nodes];
    let mut iterations = 0;
    while iterations < options.max_iterations {
        iterations += 1;
        next.fill(0.0);
        let mut dangling = 0.0;
        for (node, rank) in ranks.iter().enumerate() {
            let out = graph.out_weight[node];
            if out > 0.0 {
                for (target, weight) in graph.neighbours(node) {
                    next[target] += damping * rank * weight / out;
                }
            } else {
                dangling += rank;
            }
        }
        let restart_mass = (1.0 - damping) + damping * dangling;
        let mut change = 0.0;
        for node in 0..nodes {
            next[node] += restart_mass * restart[node];
            change += (next[node] - ranks[node]).abs();
        }
        std::mem::swap(&mut ranks, &mut next);
        if change < options.tolerance {
            break;
        }
    }
    Ranking { ranks, iterations }
}

fn normalised_restart(nodes: usize, personalisation: Option<&[f64]>) -> Vec<f64> {
    let uniform = || vec![1.0 / count(nodes); nodes];
    let Some(weights) = personalisation.filter(|weights| weights.len() == nodes) else {
        return uniform();
    };
    let cleaned: Vec<f64> = weights
        .iter()
        .map(|weight| {
            if weight.is_finite() && *weight > 0.0 {
                *weight
            } else {
                0.0
            }
        })
        .collect();
    let total: f64 = cleaned.iter().sum();
    if total > 0.0 {
        cleaned.into_iter().map(|weight| weight / total).collect()
    } else {
        uniform()
    }
}

fn index(node: u32) -> usize {
    usize::try_from(node).unwrap_or(usize::MAX)
}

#[allow(
    clippy::cast_precision_loss,
    reason = "node counts stay far below 2^52"
)]
fn count(nodes: usize) -> f64 {
    nodes as f64
}

#[cfg(test)]
mod tests {
    use super::{Graph, GraphBuilder, PageRankOptions, pagerank};

    #[test]
    fn round_trips_through_bytes_and_builders() {
        let mut builder = GraphBuilder::new(4);
        builder.add_edge(0, 1, 1.5);
        builder.add_edge(1, 2, 0.25);
        builder.add_edge(3, 0, 2.0);
        let graph = builder.build();
        let bytes = graph.to_bytes();
        assert_eq!(Graph::from_bytes(&bytes), Some(graph.clone()));
        assert_eq!(Graph::from_bytes(&bytes[..bytes.len() - 1]), None);
        assert_eq!(Graph::from_bytes(b"garbage"), None);
        let mut extended = graph.to_builder();
        extended.add_edge(2, 3, 1.0);
        let extended = extended.build();
        assert_eq!(extended.edges(), 4);
        assert_eq!(extended.neighbours(0).collect::<Vec<_>>(), [(1, 1.5)]);
    }

    #[test]
    fn merges_parallel_edges_and_drops_invalid_ones() {
        let mut builder = GraphBuilder::new(3);
        builder.add_edge(0, 1, 1.0);
        builder.add_edge(0, 1, 2.0);
        builder.add_edge(0, 2, 0.0);
        builder.add_edge(0, 5, 1.0);
        builder.add_edge(2, 0, f64::NAN);
        let graph = builder.build();
        assert_eq!(graph.edges(), 1);
        assert_eq!(graph.neighbours(0).collect::<Vec<_>>(), [(1, 3.0)]);
        assert_eq!(graph.neighbours(2).count(), 0);
        assert_eq!(graph.neighbours(9).count(), 0);
    }

    #[test]
    fn a_hub_outranks_its_leaves() {
        let mut builder = GraphBuilder::new(4);
        for leaf in 1..4 {
            builder.add_edge(leaf, 0, 1.0);
        }
        let ranking = pagerank(&builder.build(), None, &PageRankOptions::default());
        assert!(ranking.ranks[0] > ranking.ranks[1]);
        assert!((ranking.ranks.iter().sum::<f64>() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn personalisation_pulls_rank_towards_chosen_nodes() {
        let mut builder = GraphBuilder::new(3);
        builder.add_edge(0, 1, 1.0);
        builder.add_edge(1, 2, 1.0);
        builder.add_edge(2, 0, 1.0);
        let graph = builder.build();
        let uniform = pagerank(&graph, None, &PageRankOptions::default());
        let biased = pagerank(&graph, Some(&[0.0, 0.0, 1.0]), &PageRankOptions::default());
        assert!((uniform.ranks[0] - uniform.ranks[2]).abs() < 1e-6);
        assert!(biased.ranks[2] > biased.ranks[0]);
        assert!(biased.ranks[2] > uniform.ranks[2]);
    }

    #[test]
    fn empty_graph_has_no_ranks() {
        let ranking = pagerank(
            &GraphBuilder::new(0).build(),
            None,
            &PageRankOptions::default(),
        );
        assert_eq!(ranking.ranks, Vec::<f64>::new());
    }
}
