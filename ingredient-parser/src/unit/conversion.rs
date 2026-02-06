//! Unit conversion graph for ingredient measurements.
//!
//! This module provides graph-based conversion between different measurement units
//! using user-provided mappings (e.g., "1 cup flour = 120g"). The conversion algorithm
//! finds the shortest path in the conversion graph to transform between units.

use std::collections::HashMap;

use super::{kind::MeasureKind, measure::Measure, Unit};
use crate::util::round_to_int;
use petgraph::graph::NodeIndex;
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
/// * `mappings` - Slice of (from, to) measurement pairs
///
/// # Returns
/// A directed graph where nodes are units and edges are conversion factors
pub fn make_graph(mappings: &[(Measure, Measure)]) -> MeasureGraph {
    let mut g = Graph::<Unit, f64>::new();
    let mut unit_index: HashMap<Unit, NodeIndex> = HashMap::new();

    for (m_a, m_b) in mappings.iter() {
        let m_a = m_a.normalize();
        let m_b = m_b.normalize();

        let unit_a = m_a.unit().clone().normalize();
        let unit_b = m_b.unit().clone().normalize();

        let n_a = *unit_index
            .entry(unit_a.clone())
            .or_insert_with(|| g.add_node(unit_a));
        let n_b = *unit_index
            .entry(unit_b.clone())
            .or_insert_with(|| g.add_node(unit_b));

        let a_val = m_a.value();
        let b_val = m_b.value();
        let a_to_b_weight = crate::util::truncate_3_decimals(b_val / a_val);

        let exists = g
            .find_edge(n_a, n_b)
            .and_then(|e| g.edge_weight(e))
            .is_some_and(|w| *w == a_to_b_weight);
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

/// Detect disconnected components (islands) in the unit conversion graph.
///
/// Returns a list of connected components, where each component is a list of unit strings.
/// Single-node components are filtered out since they can't form meaningful conversion paths.
///
/// # Arguments
/// * `graph` - The conversion graph built from unit mappings
///
/// # Returns
/// A vector of components, where each component is a vector of unit strings
pub fn find_connected_components(graph: &MeasureGraph) -> Vec<Vec<String>> {
    use petgraph::algo::kosaraju_scc;

    // Get strongly connected components (for directed graph)
    // Since our graph is bidirectional, this effectively gives us connected components
    let components = kosaraju_scc(graph);

    // Convert node indices to unit strings
    let result: Vec<Vec<String>> = components
        .into_iter()
        .map(|component| {
            component
                .into_iter()
                .filter_map(|node_idx| graph.node_weight(node_idx).map(|unit| unit.to_string()))
                .collect()
        })
        .filter(|component: &Vec<String>| component.len() > 1) // Filter out single-node components
        .collect();

    result
}

/// Convert a measure to a target kind using a pre-built conversion graph.
///
/// This uses the A* algorithm to find the shortest path in the conversion graph
/// from the source unit to a unit of the target kind. The conversion factor
/// is computed by multiplying all edge weights along the path.
///
/// Use this when converting multiple measures to avoid rebuilding the graph
/// each time. Build the graph once with [`make_graph`] and reuse it.
///
/// # Arguments
/// * `measure` - The measure to convert
/// * `target` - The target measurement kind (e.g., Weight, Volume)
/// * `graph` - A pre-built conversion graph from [`make_graph`]
///
/// # Returns
/// `Some(converted_measure)` if a conversion path exists, `None` otherwise
#[tracing::instrument]
pub fn convert_measure_with_graph(
    measure: &Measure,
    target: MeasureKind,
    graph: &MeasureGraph,
) -> Option<Measure> {
    let input = measure.normalize();
    let unit_a = input.unit().clone();
    let unit_b = target.unit();

    let n_a = graph.node_indices().find(|i| graph[*i] == unit_a)?;
    let n_b = graph.node_indices().find(|i| graph[*i] == unit_b)?;

    debug!("calculating {:?} to {:?}", n_a, n_b);
    let Some((_, steps)) =
        petgraph::algo::astar(graph, n_a, |finish| finish == n_b, |e| *e.weight(), |_| 0.0)
    else {
        debug!("convert failed for {:?}", input);
        return None;
    };
    let mut factor: f64 = 1.0;
    for x in 0..steps.len() - 1 {
        let edge = graph.find_edge(*steps.get(x)?, *steps.get(x + 1)?)?;
        factor *= graph.edge_weight(edge)?;
    }

    let input_val = input.value();
    let input_upper = input.upper_value();
    let result = Measure::new_with_upper(
        unit_b,
        round_to_int(input_val * factor),
        input_upper.map(|x| round_to_int(x * factor)),
    );
    debug!("{:?} -> {:?} ({} hops)", input, result, steps.len());
    Some(result.denormalize())
}

/// Convert a measure to a target kind using user-provided mappings.
///
/// Convenience wrapper that builds the graph and converts in one call.
/// If converting multiple measures, prefer [`convert_measure_with_graph`]
/// with a pre-built graph from [`make_graph`].
///
/// # Arguments
/// * `measure` - The measure to convert
/// * `target` - The target measurement kind (e.g., Weight, Volume)
/// * `mappings` - Slice of known conversions between measurements
///
/// # Returns
/// `Some(converted_measure)` if a conversion path exists, `None` otherwise
pub fn convert_measure_via_mappings(
    measure: &Measure,
    target: MeasureKind,
    mappings: &[(Measure, Measure)],
) -> Option<Measure> {
    let g = make_graph(mappings);
    convert_measure_with_graph(measure, target, &g)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_make_graph_basic() {
        let mappings = vec![(Measure::new("cup", 1.0), Measure::new("g", 120.0))];

        let graph = make_graph(&mappings);

        assert_eq!(graph.node_count(), 2);
        assert_eq!(graph.edge_count(), 2);
    }

    #[test]
    fn test_make_graph_duplicate_edges() {
        let mappings = vec![
            (Measure::new("cup", 1.0), Measure::new("g", 120.0)),
            (Measure::new("cup", 1.0), Measure::new("g", 120.0)),
        ];

        let graph = make_graph(&mappings);

        assert_eq!(graph.edge_count(), 2);
    }

    #[test]
    fn test_make_graph_different_weights() {
        let mappings = vec![
            (Measure::new("cup", 1.0), Measure::new("g", 120.0)),
            (Measure::new("cup", 2.0), Measure::new("g", 240.0)),
        ];

        let graph = make_graph(&mappings);

        assert_eq!(graph.edge_count(), 2);
    }

    #[test]
    fn test_print_graph() {
        let mappings = vec![(Measure::new("cup", 1.0), Measure::new("g", 120.0))];

        let graph = make_graph(&mappings);
        let dot = print_graph(graph);

        assert!(dot.contains("digraph"));
    }

    #[test]
    fn test_convert_measure_success() {
        let measure = Measure::new("cup", 2.0);
        let mappings = vec![(Measure::new("cup", 1.0), Measure::new("g", 120.0))];

        let result = convert_measure_via_mappings(&measure, MeasureKind::Weight, &mappings);

        assert!(result.is_some());
        let converted = result.unwrap();
        assert_eq!(converted.value(), 240.0);
    }

    #[test]
    fn test_convert_measure_no_source_node() {
        let measure = Measure::new("pinch", 1.0);
        let mappings = vec![(Measure::new("cup", 1.0), Measure::new("g", 120.0))];

        let result = convert_measure_via_mappings(&measure, MeasureKind::Weight, &mappings);

        assert!(result.is_none());
    }

    #[test]
    fn test_convert_measure_no_target_node() {
        let measure = Measure::new("cup", 1.0);
        let mappings = vec![(Measure::new("cup", 1.0), Measure::new("g", 120.0))];

        let result = convert_measure_via_mappings(&measure, MeasureKind::Money, &mappings);

        assert!(result.is_none());
    }

    #[test]
    fn test_convert_measure_no_path() {
        let mappings = vec![
            (Measure::new("cup", 1.0), Measure::new("ml", 240.0)),
            (Measure::new("dollar", 1.0), Measure::new("cent", 100.0)),
        ];

        let measure = Measure::new("cup", 1.0);
        let result = convert_measure_via_mappings(&measure, MeasureKind::Money, &mappings);

        assert!(result.is_none());
    }

    #[test]
    fn test_convert_measure_with_range() {
        let measure = Measure::with_range("cup", 1.0, 2.0);
        let mappings = vec![(Measure::new("cup", 1.0), Measure::new("g", 120.0))];

        let result = convert_measure_via_mappings(&measure, MeasureKind::Weight, &mappings);

        assert!(result.is_some());
        let converted = result.unwrap();
        assert_eq!(converted.value(), 120.0);
        assert_eq!(converted.upper_value(), Some(240.0));
    }

    #[test]
    fn test_convert_measure_multi_hop() {
        let mappings = vec![
            (Measure::new("cup", 1.0), Measure::new("tbsp", 16.0)),
            (Measure::new("tbsp", 1.0), Measure::new("g", 15.0)),
        ];

        let measure = Measure::new("cup", 1.0);
        let result = convert_measure_via_mappings(&measure, MeasureKind::Weight, &mappings);

        assert!(result.is_some());
        let converted = result.unwrap();
        assert_eq!(converted.value(), 240.0);
    }

    #[test]
    fn test_convert_measure_with_graph_reuse() {
        let mappings = vec![(Measure::new("cup", 1.0), Measure::new("g", 120.0))];
        let graph = make_graph(&mappings);

        // Convert multiple measures using the same graph
        let m1 = Measure::new("cup", 1.0);
        let m2 = Measure::new("cup", 3.0);

        let r1 = convert_measure_with_graph(&m1, MeasureKind::Weight, &graph);
        let r2 = convert_measure_with_graph(&m2, MeasureKind::Weight, &graph);

        assert_eq!(r1.unwrap().value(), 120.0);
        assert_eq!(r2.unwrap().value(), 360.0);
    }
}
