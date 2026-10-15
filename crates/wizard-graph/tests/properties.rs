//! Invariants of personalised `PageRank` on random graphs.

#![allow(clippy::unwrap_used)]

use proptest::prelude::{Strategy, prop, proptest};
use wizard_graph::{Graph, GraphBuilder, PageRankOptions, pagerank};

fn graph() -> impl Strategy<Value = (Graph, Vec<f64>)> {
    (1_usize..40).prop_flat_map(|nodes| {
        (
            prop::collection::vec((0..nodes, 0..nodes, 0.01_f64..10.0), 0..200),
            prop::collection::vec(0.0_f64..5.0, nodes),
        )
            .prop_map(move |(edges, personalisation)| {
                let mut builder = GraphBuilder::new(nodes);
                for (from, to, weight) in edges {
                    builder.add_edge(from, to, weight);
                }
                (builder.build(), personalisation)
            })
    })
}

proptest! {
    #[test]
    fn ranks_are_a_probability_distribution((graph, personalisation) in graph()) {
        let ranking = pagerank(&graph, Some(&personalisation), &PageRankOptions::default());
        assert_eq!(ranking.ranks.len(), graph.nodes());
        assert!(ranking.ranks.iter().all(|rank| rank.is_finite() && *rank >= 0.0));
        assert!((ranking.ranks.iter().sum::<f64>() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn results_are_deterministic((graph, personalisation) in graph()) {
        let options = PageRankOptions::default();
        assert_eq!(
            pagerank(&graph, Some(&personalisation), &options),
            pagerank(&graph, Some(&personalisation), &options)
        );
    }

    #[test]
    fn iterations_respect_the_cap((graph, personalisation) in graph()) {
        let options = PageRankOptions { max_iterations: 7, ..PageRankOptions::default() };
        assert!(pagerank(&graph, Some(&personalisation), &options).iterations <= 7);
    }

    #[test]
    fn restart_only_nodes_rank_at_least_their_restart_share((graph, personalisation) in graph()) {
        let options = PageRankOptions::default();
        let ranking = pagerank(&graph, Some(&personalisation), &options);
        let total: f64 = personalisation.iter().sum();
        if total > 0.0 {
            for (rank, weight) in ranking.ranks.iter().zip(&personalisation) {
                assert!(*rank + 1e-9 >= (1.0 - options.damping) * weight / total);
            }
        }
    }
}
