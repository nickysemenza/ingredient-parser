//! Unit conversion graph for ingredient measurements.
//!
//! This module provides graph-based conversion between different measurement units
//! using user-provided mappings (e.g., "1 cup flour = 120g"). The conversion algorithm
//! finds the shortest path in the conversion graph to transform between units.

use std::collections::HashMap;

use super::{kind::MeasureKind, measure::Measure, measure::TSP_TO_ML, Unit};
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
        // Full-precision factors. The conversion result is integer-rounded at
        // the end, so truncating to 3 decimals here only compounded drift over
        // multi-hop paths without changing the rounded output.
        let a_to_b_weight = b_val / a_val;
        let b_to_a_weight = a_val / b_val;

        match g.find_edge(n_a, n_b) {
            // Edge already present. If the weight conflicts (e.g. both
            // "1 cup = 120 g" and "1 cup = 130 g" were supplied), update it in
            // place — latest mapping wins — rather than adding a *parallel* edge
            // that fewest-hops A* would then pick between nondeterministically.
            Some(e) => {
                if g.edge_weight(e).is_some_and(|w| *w != a_to_b_weight) {
                    debug!(
                        "conflicting mapping {:?}->{:?}, using latest weight {}",
                        m_a.unit(),
                        m_b.unit(),
                        a_to_b_weight
                    );
                    if let Some(w) = g.edge_weight_mut(e) {
                        *w = a_to_b_weight;
                    }
                    if let Some(re) = g.find_edge(n_b, n_a) {
                        if let Some(rw) = g.edge_weight_mut(re) {
                            *rw = b_to_a_weight;
                        }
                    }
                }
            }
            None => {
                g.add_edge(n_a, n_b, a_to_b_weight);
                g.add_edge(n_b, n_a, b_to_a_weight);
            }
        }
    }

    // Bridge the two volume normalization bases (teaspoon for the US/spoon family,
    // milliliter for the metric family). Without this edge the families are disconnected,
    // so a US-volume-only graph can't reach the `ml` node that `MeasureKind::Volume`
    // targets (the "1 cup -> Volume: not convertible" bug). The ratio is a fixed
    // geometric constant, density-independent. Only seed when a volume unit is already
    // present, so unrelated graphs (and the graph viz / island detector) gain no stray
    // nodes.
    if unit_index.contains_key(&Unit::Teaspoon) || unit_index.contains_key(&Unit::Milliliter) {
        let n_tsp = *unit_index
            .entry(Unit::Teaspoon)
            .or_insert_with(|| g.add_node(Unit::Teaspoon));
        let n_ml = *unit_index
            .entry(Unit::Milliliter)
            .or_insert_with(|| g.add_node(Unit::Milliliter));
        if g.find_edge(n_tsp, n_ml).is_none() {
            g.add_edge(n_tsp, n_ml, TSP_TO_ML);
            g.add_edge(n_ml, n_tsp, 1.0 / TSP_TO_ML);
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
/// Returns the connected components, each a list of unit strings. Single-node
/// components are dropped since they can't form a meaningful conversion path.
/// The graph is bidirectional, so strongly-connected components coincide with
/// connected components.
///
/// # Arguments
/// * `graph` - A conversion graph built from [`make_graph`]
///
/// # Returns
/// A vector of components, each a vector of unit strings
pub fn find_connected_components(graph: &MeasureGraph) -> Vec<Vec<String>> {
    petgraph::algo::kosaraju_scc(graph)
        .into_iter()
        .map(|component| {
            component
                .into_iter()
                .filter_map(|node_idx| graph.node_weight(node_idx).map(|unit| unit.to_string()))
                .collect::<Vec<String>>()
        })
        .filter(|component| component.len() > 1)
        .collect()
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
    // Edge cost is a uniform 1.0 (fewest hops), NOT the edge weight. The weight is
    // the conversion *factor*, and the result is the PRODUCT of factors along the
    // path (below) — so minimizing their SUM would optimize the wrong quantity. It
    // could prefer a cheap-sum indirect route (or a chain of <1 reverse edges) over
    // a direct user mapping, returning a derived value instead of the authoritative
    // one and compounding rounding. In a consistent graph every path yields the same
    // product, so fewest hops is correct and minimizes multiplicative drift.
    let Some((_, steps)) =
        petgraph::algo::astar(graph, n_a, |finish| finish == n_b, |_| 1.0, |_| 0.0)
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
        (input_val * factor).round(),
        input_upper.map(|x| (x * factor).round()),
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
pub(crate) fn convert_measure_via_mappings(
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

        // cup normalizes to the tsp node, which triggers the tsp<->ml volume bridge:
        // nodes {tsp, g, ml}, edges tsp<->g and tsp<->ml.
        assert_eq!(graph.node_count(), 3);
        assert_eq!(graph.edge_count(), 4);
    }

    #[test]
    fn test_make_graph_duplicate_edges() {
        let mappings = vec![
            (Measure::new("cup", 1.0), Measure::new("g", 120.0)),
            (Measure::new("cup", 1.0), Measure::new("g", 120.0)),
        ];

        let graph = make_graph(&mappings);

        // 2 for tsp<->g (duplicate mapping deduped) + 2 for the tsp<->ml bridge.
        assert_eq!(graph.edge_count(), 4);
    }

    #[test]
    fn test_make_graph_different_weights() {
        let mappings = vec![
            (Measure::new("cup", 1.0), Measure::new("g", 120.0)),
            (Measure::new("cup", 2.0), Measure::new("g", 240.0)),
        ];

        let graph = make_graph(&mappings);

        // Both mappings collapse to the same tsp<->g edge weight + the tsp<->ml bridge.
        assert_eq!(graph.edge_count(), 4);
    }

    #[test]
    fn test_make_graph_volume_bridge_seeded_when_volume_present() {
        // A US-volume mapping gains the metric `ml` node via the bridge.
        let mappings = vec![(Measure::new("cup", 1.0), Measure::new("g", 120.0))];
        let graph = make_graph(&mappings);
        assert!(graph.node_indices().any(|i| graph[i] == Unit::Milliliter));
        assert!(graph.node_indices().any(|i| graph[i] == Unit::Teaspoon));
    }

    #[test]
    fn test_make_graph_no_bridge_without_volume() {
        // No volume unit -> no stray tsp/ml nodes seeded.
        let mappings = vec![(Measure::new("g", 1.0), Measure::new("$", 8.0))];
        let graph = make_graph(&mappings);
        assert_eq!(graph.node_count(), 2);
        assert_eq!(graph.edge_count(), 2);
        assert!(!graph.node_indices().any(|i| graph[i] == Unit::Teaspoon));
        assert!(!graph.node_indices().any(|i| graph[i] == Unit::Milliliter));
    }

    #[test]
    fn test_convert_cup_to_volume_via_bridge() {
        // The core bug: 1 cup should be convertible to Volume (ml) even though the only
        // mapping is a density-dependent cup->g. ~236.6 ml.
        let mappings = vec![(Measure::new("cup", 1.0), Measure::new("g", 120.0))];
        let measure = Measure::new("cup", 1.0);
        let result = convert_measure_via_mappings(&measure, MeasureKind::Volume, &mappings);
        assert!(result.is_some());
        let ml = result.unwrap();
        assert_eq!(*ml.unit(), Unit::Milliliter);
        assert!((ml.value() - 236.0).abs() < 2.0, "got {}", ml.value());
    }

    #[test]
    fn test_convert_metric_volume_still_works() {
        // Metric-only volume mapping: ml node already present, bridge adds tsp but the
        // direct ml result is unchanged.
        let mappings = vec![(Measure::new("ml", 100.0), Measure::new("g", 90.0))];
        let measure = Measure::new("ml", 250.0);
        let result = convert_measure_via_mappings(&measure, MeasureKind::Volume, &mappings);
        assert!(result.is_some());
        assert_eq!(result.unwrap().value(), 250.0);
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

    #[test]
    fn test_convert_prefers_direct_mapping_over_indirect() {
        // Conflicting mappings: a direct widget->g (10) and an indirect
        // widget->blob->g (2 * 6 = 12). The indirect route has a cheaper edge-weight
        // SUM (2 + 6 = 8 < 10), so cost-by-weight A* would wrongly pick it and return
        // 12 g. Cost-by-hops picks the 1-hop direct mapping → the authoritative 10 g.
        // Custom (Other) units: no normalization, no volume bridge in play.
        let mappings = vec![
            (Measure::new("widget", 1.0), Measure::new("g", 10.0)),
            (Measure::new("widget", 1.0), Measure::new("blob", 2.0)),
            (Measure::new("blob", 1.0), Measure::new("g", 6.0)),
        ];
        let result = convert_measure_via_mappings(
            &Measure::new("widget", 1.0),
            MeasureKind::Weight,
            &mappings,
        );
        assert_eq!(result.unwrap().value(), 10.0);
    }

    #[test]
    fn test_conflicting_mapping_updates_in_place_last_wins() {
        // Two conflicting mappings for the same pair must NOT create a parallel
        // edge (which fewest-hops A* would pick between nondeterministically).
        // The latest mapping wins. Custom units avoid the volume bridge.
        let mappings = vec![
            (Measure::new("widget", 1.0), Measure::new("g", 10.0)),
            (Measure::new("widget", 1.0), Measure::new("g", 13.0)),
        ];
        let graph = make_graph(&mappings);
        // Exactly one edge each way, not two parallel ones.
        assert_eq!(graph.edge_count(), 2);
        let result = convert_measure_via_mappings(
            &Measure::new("widget", 1.0),
            MeasureKind::Weight,
            &mappings,
        );
        assert_eq!(result.unwrap().value(), 13.0);
    }

    #[test]
    fn test_convert_not_lured_through_cheap_reverse_edges() {
        // A direct widget->g (1000) vs a multi-hop chain of sub-1 weights that sums
        // far cheaper but is the wrong route. Fewest-hops must still take the single
        // direct edge (1000 g), not the longer cheap-sum detour.
        let mappings = vec![
            (Measure::new("widget", 1.0), Measure::new("g", 1000.0)),
            (Measure::new("widget", 1.0), Measure::new("blob", 0.5)),
            (Measure::new("blob", 1.0), Measure::new("speck", 0.5)),
            (Measure::new("speck", 1.0), Measure::new("g", 0.5)),
        ];
        let result = convert_measure_via_mappings(
            &Measure::new("widget", 1.0),
            MeasureKind::Weight,
            &mappings,
        );
        assert_eq!(result.unwrap().value(), 1000.0);
    }
}
