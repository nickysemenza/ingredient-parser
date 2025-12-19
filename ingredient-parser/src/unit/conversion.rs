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
