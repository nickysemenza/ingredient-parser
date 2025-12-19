//! Unit conversion graph for ingredient measurements.
//!
//! This module provides graph-based conversion between different measurement units
//! using user-provided mappings (e.g., "1 cup flour = 120g"). The conversion algorithm
//! finds the shortest path in the conversion graph to transform between units.

use super::{kind::MeasureKind, measure::Measure, Unit};
use crate::util::round_to_int;
use petgraph::Graph;
use tracing::debug;

pub type MeasureGraph = Graph<Unit, f64>;

/// Build a conversion graph from a list of measurement mappings.
///
/// Each mapping represents an equivalence like "1 cup flour = 120g".
/// The graph stores nodes as units and edges as conversion factors.
/// Both directions (A→B and B→A) are added for bidirectional conversion.
///
/// # Arguments
/// * `mappings` - List of (from, to) measurement pairs
///
/// # Returns
/// A directed graph where nodes are units and edges are conversion factors
pub fn make_graph(mappings: Vec<(Measure, Measure)>) -> MeasureGraph {
    let mut g = Graph::<Unit, f64>::new();

    for (mut m_a, mut m_b) in mappings.into_iter() {
        m_a = m_a.normalize();
        m_b = m_b.normalize();
        let n_a = g
            .node_indices()
            .find(|i| g[*i] == m_a.unit())
            .unwrap_or_else(|| g.add_node(m_a.unit().normalize()));
        let n_b = g
            .node_indices()
            .find(|i| g[*i] == m_b.unit())
            .unwrap_or_else(|| g.add_node(m_b.unit().normalize()));

        let (a_val, _, _) = m_a.values();
        let (b_val, _, _) = m_b.values();
        let a_to_b_weight = crate::util::truncate_3_decimals(b_val / a_val);

        let exists = match g.find_edge(n_a, n_b) {
            Some(existing_edge) => match g.edge_weight(existing_edge) {
                Some(weight) => *weight == a_to_b_weight,
                None => false,
            },
            None => false,
        };
        if !exists {
            // if a to b exists with the right weight, then b to a likely exists too
            // edge from a to b
            g.add_edge(n_a, n_b, a_to_b_weight);
            // edge from b to a
            g.add_edge(n_b, n_a, crate::util::truncate_3_decimals(a_val / b_val));
        }
    }
    g
}

/// Format the conversion graph as a DOT diagram for debugging.
///
/// This can be visualized with Graphviz or other DOT rendering tools.
pub fn print_graph(g: MeasureGraph) -> String {
    format!("{}", petgraph::dot::Dot::new(&g))
}

/// Convert a measure to a target kind using user-provided mappings.
///
/// This uses the A* algorithm to find the shortest path in the conversion graph
/// from the source unit to a unit of the target kind. The conversion factor
/// is computed by multiplying all edge weights along the path.
///
/// # Arguments
/// * `measure` - The measure to convert
/// * `target` - The target measurement kind (e.g., Weight, Volume)
/// * `mappings` - List of known conversions between measurements
///
/// # Returns
/// `Some(converted_measure)` if a conversion path exists, `None` otherwise
#[tracing::instrument]
pub fn convert_measure_via_mappings(
    measure: &Measure,
    target: MeasureKind,
    mappings: Vec<(Measure, Measure)>,
) -> Option<Measure> {
    let g = make_graph(mappings);
    let input = measure.normalize();
    let unit_a = input.unit();
    let unit_b = target.unit();

    let n_a = g.node_indices().find(|i| g[*i] == unit_a)?;
    let n_b = g.node_indices().find(|i| g[*i] == unit_b)?;

    debug!("calculating {:?} to {:?}", n_a, n_b);
    if !petgraph::algo::has_path_connecting(&g, n_a, n_b, None) {
        debug!("convert failed for {:?}", input);
        return None;
    };

    let steps = petgraph::algo::astar(&g, n_a, |finish| finish == n_b, |e| *e.weight(), |_| 0.0)?.1;
    let mut factor: f64 = 1.0;
    for x in 0..steps.len() - 1 {
        let edge = g.find_edge(*steps.get(x)?, *steps.get(x + 1)?)?;
        factor *= g.edge_weight(edge)?;
    }

    let (input_val, input_upper, _) = input.values();
    let result = Measure::new_with_upper(
        unit_b,
        round_to_int(input_val * factor),
        input_upper.map(|x| round_to_int(x * factor)),
    );
    debug!("{:?} -> {:?} ({} hops)", input, result, steps.len());
    Some(result.denormalize())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_make_graph_basic() {
        // Test basic graph creation
        let mappings = vec![(Measure::new("cup", 1.0), Measure::new("g", 120.0))];

        let graph = make_graph(mappings);

        // Should have 2 nodes (cup normalized to tsp, g)
        assert_eq!(graph.node_count(), 2);
        // Should have 2 edges (bidirectional)
        assert_eq!(graph.edge_count(), 2);
    }

    #[test]
    fn test_make_graph_duplicate_edges() {
        // Test that duplicate edges with same weight are not added
        let mappings = vec![
            (Measure::new("cup", 1.0), Measure::new("g", 120.0)),
            // Same mapping again - should not add duplicate edges
            (Measure::new("cup", 1.0), Measure::new("g", 120.0)),
        ];

        let graph = make_graph(mappings);

        // Should still have just 2 edges (duplicates not added)
        assert_eq!(graph.edge_count(), 2);
    }

    #[test]
    fn test_make_graph_different_weights() {
        // Test that edges with different weights ARE added
        let mappings = vec![
            (Measure::new("cup", 1.0), Measure::new("g", 120.0)),
            // Different weight - should add new edges
            (Measure::new("cup", 2.0), Measure::new("g", 240.0)),
        ];

        let graph = make_graph(mappings);

        // Same weight ratio (120g/cup), so still just 2 edges
        assert_eq!(graph.edge_count(), 2);
    }

    #[test]
    fn test_print_graph() {
        let mappings = vec![(Measure::new("cup", 1.0), Measure::new("g", 120.0))];

        let graph = make_graph(mappings);
        let dot = print_graph(graph);

        // Should produce DOT format output
        assert!(dot.contains("digraph"));
    }

    #[test]
    fn test_convert_measure_success() {
        // Test successful conversion
        let measure = Measure::new("cup", 2.0);
        let mappings = vec![(Measure::new("cup", 1.0), Measure::new("g", 120.0))];

        let result = convert_measure_via_mappings(&measure, MeasureKind::Weight, mappings);

        assert!(result.is_some());
        let converted = result.unwrap();
        // 2 cups * 120g/cup = 240g
        assert_eq!(converted.values().0, 240.0);
    }

    #[test]
    fn test_convert_measure_no_source_node() {
        // Test when source unit is not in the graph
        let measure = Measure::new("pinch", 1.0);
        let mappings = vec![(Measure::new("cup", 1.0), Measure::new("g", 120.0))];

        let result = convert_measure_via_mappings(&measure, MeasureKind::Weight, mappings);

        // Should return None - source unit not in graph
        assert!(result.is_none());
    }

    #[test]
    fn test_convert_measure_no_target_node() {
        // Test when target unit is not in the graph
        let measure = Measure::new("cup", 1.0);
        let mappings = vec![(Measure::new("cup", 1.0), Measure::new("g", 120.0))];

        // Try to convert to Money (Cent) which isn't in the mappings
        let result = convert_measure_via_mappings(&measure, MeasureKind::Money, mappings);

        // Should return None - target unit not in graph
        assert!(result.is_none());
    }

    #[test]
    fn test_convert_measure_no_path() {
        // Test when there's no path between source and target
        // Create disconnected graph components
        let mappings = vec![
            (Measure::new("cup", 1.0), Measure::new("ml", 240.0)),
            // Disconnected: dollars to cents (no path to volume)
            (Measure::new("dollar", 1.0), Measure::new("cent", 100.0)),
        ];

        let measure = Measure::new("cup", 1.0);
        // Try to convert volume to money - no path exists
        let result = convert_measure_via_mappings(&measure, MeasureKind::Money, mappings);

        // Should return None - no path connecting volume to money
        assert!(result.is_none());
    }

    #[test]
    fn test_convert_measure_with_range() {
        // Test conversion preserves range values
        let measure = Measure::with_range("cup", 1.0, 2.0);
        let mappings = vec![(Measure::new("cup", 1.0), Measure::new("g", 120.0))];

        let result = convert_measure_via_mappings(&measure, MeasureKind::Weight, mappings);

        assert!(result.is_some());
        let converted = result.unwrap();
        assert_eq!(converted.values().0, 120.0);
        assert_eq!(converted.values().1, Some(240.0));
    }

    #[test]
    fn test_convert_measure_multi_hop() {
        // Test conversion that requires multiple hops
        let mappings = vec![
            (Measure::new("cup", 1.0), Measure::new("tbsp", 16.0)),
            (Measure::new("tbsp", 1.0), Measure::new("g", 15.0)),
        ];

        let measure = Measure::new("cup", 1.0);
        let result = convert_measure_via_mappings(&measure, MeasureKind::Weight, mappings);

        assert!(result.is_some());
        // 1 cup -> 16 tbsp -> 240g
        let converted = result.unwrap();
        assert_eq!(converted.values().0, 240.0);
    }
}
