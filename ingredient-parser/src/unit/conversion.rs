//! Unit conversion graph for ingredient measurements.
//!
//! This module provides graph-based conversion between different measurement units
//! using user-provided mappings (e.g., "1 cup flour = 120g"). The conversion algorithm
//! finds the shortest path in the conversion graph to transform between units.

use std::collections::HashMap;

use super::{
    Unit,
    kind::MeasureKind,
    measure::Measure,
    measure::{DASH_TO_TSP, PINCH_TO_TSP, TSP_TO_ML},
};
use petgraph::Graph;
use petgraph::graph::NodeIndex;
use tracing::debug;

pub type MeasureGraph = Graph<Unit, EdgeFactor>;

/// A directed edge's conversion factor as a closed interval `[lower, upper]`.
///
/// For an ordinary point mapping (the common case — "1 cup = 120 g", a price, a
/// USDA nutrient) `lower == upper`. A *ranged* mapping carries a genuine
/// interval: the only producer today is a sub-recipe yield expressed as a range
/// ("1 batch = $6–8" when the batch contains a ranged ingredient). Conversions
/// multiply the interval along the path, so the range propagates to the result.
///
/// Inversion (the reverse edge) and a ranged *source* measure both use positive
/// interval division `[a,b] / [c,d] = [a/d, b/c]` — note the bound flip — so a
/// conversion that traverses a ranged edge backward stays correct.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EdgeFactor {
    pub lower: f64,
    pub upper: f64,
}

impl EdgeFactor {
    /// A degenerate (point) factor where both bounds coincide — every
    /// non-ranged mapping and the synthesized volume bridge.
    pub fn point(f: f64) -> Self {
        Self { lower: f, upper: f }
    }
}

impl std::fmt::Display for EdgeFactor {
    /// Point factors render as a bare number; ranged factors as `lo–hi`. Keeps
    /// the DOT graph viz (`print_graph`) readable.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.lower == self.upper {
            write!(f, "{}", self.lower)
        } else {
            write!(f, "{}–{}", self.lower, self.upper)
        }
    }
}

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
    let mut g = Graph::<Unit, EdgeFactor>::new();
    let mut unit_index: HashMap<Unit, NodeIndex> = HashMap::new();

    for (m_a, m_b) in mappings.iter() {
        let m_a = m_a.normalize();
        let m_b = m_b.normalize();

        let unit_a = m_a.unit().normalize();
        let unit_b = m_b.unit().normalize();

        let n_a = *unit_index
            .entry(unit_a.clone())
            .or_insert_with(|| g.add_node(unit_a));
        let n_b = *unit_index
            .entry(unit_b.clone())
            .or_insert_with(|| g.add_node(unit_b));

        // Full-precision interval factors. Point mappings collapse to
        // lower == upper; a ranged mapping yields a genuine interval. Interval
        // division [b]/[a] with all-positive bounds: lower = b_lo/a_hi,
        // upper = b_hi/a_lo (the inverse flips the bounds). The conversion
        // result is integer-rounded at the end, so full precision here only
        // avoids compounding drift over multi-hop paths.
        //
        // Assumes strictly-positive bounds (the prior scalar code did too): a 0
        // mapping value yields inf/NaN factors. Not asserted because a legitimate
        // $0 mapping shouldn't panic; ranged mappings (sub-recipe yields) always
        // carry a positive lower bound, so the reciprocal stays finite.
        let a_lo = m_a.value();
        let a_hi = m_a.upper_value().unwrap_or(a_lo);
        let b_lo = m_b.value();
        let b_hi = m_b.upper_value().unwrap_or(b_lo);
        let a_to_b_weight = EdgeFactor {
            lower: b_lo / a_hi,
            upper: b_hi / a_lo,
        };
        let b_to_a_weight = EdgeFactor {
            lower: a_lo / b_hi,
            upper: a_hi / b_lo,
        };

        match g.find_edge(n_a, n_b) {
            // Edge already present. If the weight conflicts (e.g. both
            // "1 cup = 120 g" and "1 cup = 130 g" were supplied), update it in
            // place — latest mapping wins — rather than adding a *parallel* edge
            // that fewest-hops A* would then pick between nondeterministically.
            Some(e) => {
                if g.edge_weight(e).is_some_and(|w| *w != a_to_b_weight) {
                    debug!(
                        "conflicting mapping {:?}->{:?}, using latest weight {:?}",
                        m_a.unit(),
                        m_b.unit(),
                        a_to_b_weight
                    );
                    if let Some(w) = g.edge_weight_mut(e) {
                        *w = a_to_b_weight;
                    }
                    if let Some(re) = g.find_edge(n_b, n_a)
                        && let Some(rw) = g.edge_weight_mut(re)
                    {
                        *rw = b_to_a_weight;
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
            g.add_edge(n_tsp, n_ml, EdgeFactor::point(TSP_TO_ML));
            g.add_edge(n_ml, n_tsp, EdgeFactor::point(1.0 / TSP_TO_ML));
        }
    }

    g
}

/// Round `x` to `sig` significant figures, for display only — the graph keeps
/// full-precision factors. Zero and non-finite values pass through unchanged.
fn round_sig(x: f64, sig: i32) -> f64 {
    if x == 0.0 || !x.is_finite() {
        return x;
    }
    let d = (sig - 1) - x.abs().log10().floor() as i32;
    let p = 10f64.powi(d);
    (x * p).round() / p
}

/// A conversion factor formatted for a graph label: a bare number for a point
/// factor, `lo–hi` for a range. Rounded to 3 significant figures so the viz
/// isn't buried in full-precision noise like "0.0083333333333".
fn factor_label(f: &EdgeFactor) -> String {
    let lo = round_sig(f.lower, 3);
    if f.lower == f.upper {
        format!("{lo}")
    } else {
        format!("{lo}–{}", round_sig(f.upper, 3))
    }
}

/// Render the conversion graph as a DOT diagram. Used both for ad-hoc debugging
/// and as the source for the unit-mapping graph visualization, so it makes a few
/// readability choices over a raw petgraph dump:
///
/// - The two opposing edges of each mapping (`make_graph` adds A→B and B→A)
///   collapse into one `dir=both` edge, so reciprocal labels don't overlap. The
///   label is the canonical low→high node-index direction's factor.
/// - Factors are rounded to 3 significant figures (see [`factor_label`]).
/// - Each node carries a `class` of its [`MeasureKind`] ("weight", "volume",
///   "nutrient", …) so a host can theme nodes by category.
/// - The synthesized teaspoon↔milliliter volume bridge is dashed, to read as
///   derived rather than user-provided.
// Escape `"` and `\` (and fold newlines) so a unit name like `1" cubes` can't
// terminate a DOT label early and yield invalid DOT the renderer rejects.
fn dot_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace(['\n', '\r'], " ")
}

pub fn print_graph(g: &MeasureGraph) -> String {
    use petgraph::visit::EdgeRef;
    use std::collections::HashSet;
    use std::fmt::Write as _;

    let mut out = String::from("digraph {\n");

    for idx in g.node_indices() {
        let unit = &g[idx];
        let class = match unit.kind() {
            MeasureKind::Weight => "weight",
            MeasureKind::Volume => "volume",
            MeasureKind::Money => "money",
            MeasureKind::Calories => "calories",
            MeasureKind::Time => "time",
            MeasureKind::Temperature => "temperature",
            MeasureKind::Length => "length",
            MeasureKind::Nutrient(_) => "nutrient",
            MeasureKind::Other(_) => "other",
        };
        let _ = writeln!(
            out,
            "    {} [ label = \"{}\", class = \"{class}\" ]",
            idx.index(),
            dot_escape(&unit.to_string())
        );
    }

    let mut seen: HashSet<(usize, usize)> = HashSet::new();
    for e in g.edge_references() {
        let (a, b) = (e.source().index(), e.target().index());
        let (lo, hi) = (a.min(b), a.max(b));
        if !seen.insert((lo, hi)) {
            continue;
        }
        // Label with the low→high direction's factor for determinism (iteration
        // order over the two opposing edges isn't guaranteed).
        let weight = g
            .find_edge(NodeIndex::new(lo), NodeIndex::new(hi))
            .and_then(|edge| g.edge_weight(edge))
            .copied()
            .unwrap_or_else(|| *e.weight());
        let bridge = matches!(
            (&g[NodeIndex::new(lo)], &g[NodeIndex::new(hi)]),
            (Unit::Teaspoon, Unit::Milliliter) | (Unit::Milliliter, Unit::Teaspoon)
        );
        let style = if bridge { ", style = \"dashed\"" } else { "" };
        let _ = writeln!(
            out,
            "    {lo} -> {hi} [ label = \"{}\", dir = \"both\"{style} ]",
            dot_escape(&factor_label(&weight))
        );
    }

    out.push_str("}\n");
    out
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

/// One hop of an explained conversion path: `from_unit —×factor→ to_unit`.
///
/// Units are the *normalized* graph nodes (e.g. a cup amount enters the graph
/// at the teaspoon node), so a path reads exactly as the graph traversed it.
#[derive(Debug, Clone, PartialEq)]
pub struct ConversionStep {
    pub from_unit: Unit,
    pub to_unit: Unit,
    pub factor: f64,
}

/// A tiny volumetric unit's size in teaspoons, if `unit` is one.
///
/// `unit` must already be normalized (lowercase + singular), which is how every
/// caller has it — so `"Pinches"` arrives here as `Other("pinch")`.
fn tiny_volume_in_tsp(unit: &Unit) -> Option<f64> {
    match unit {
        Unit::Other(s) => match s.as_str() {
            "pinch" => Some(PINCH_TO_TSP),
            "dash" => Some(DASH_TO_TSP),
            _ => None,
        },
        _ => None,
    }
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
pub fn convert_measure_with_graph(
    measure: &Measure,
    target: MeasureKind,
    graph: &MeasureGraph,
) -> Option<Measure> {
    convert_measure_with_graph_explained(measure, target, graph).map(|(m, _)| m)
}

/// Like [`convert_measure_with_graph`], but also returns the traversed path —
/// one [`ConversionStep`] per graph edge, in order, whose factors multiply to
/// the overall conversion factor (empty when source and target share a node).
/// This is the conversion's "show your work": it makes a wrong result
/// self-explanatory (e.g. a 34 g amount reaching money via a bogus
/// `g → whole → $` route reads right off the steps).
// `skip_all` + `level = "trace"`: this runs on the per-conversion hot path (every
// nutrient/cost/weight resolution per row per recipe). The default `#[instrument]`
// is INFO and Debug-formats ALL args — including the whole `MeasureGraph` (a
// petgraph) — into the span on every call, building a large transient String.
// Under a reused WASM isolate that allocation churn (and any subscriber state) is
// per-call overhead nobody consumes in prod. Keep the span for explain-path
// debugging but make it free when disabled: no arg formatting, below INFO.
#[tracing::instrument(level = "trace", skip_all)]
pub fn convert_measure_with_graph_explained(
    measure: &Measure,
    target: MeasureKind,
    graph: &MeasureGraph,
) -> Option<(Measure, Vec<ConversionStep>)> {
    let input = measure.normalize();
    // Normalize BOTH endpoints the exact way `make_graph` normalizes its nodes
    // (`unit().normalize()` — lowercase + singularize + promote known aliases), so
    // the node lookups below can't miss. Without this on the target, a descriptor
    // that singularizes — only `carbs` -> `carb` among the tier-1 nutrients —
    // looks up `Other("g carbs")` while the graph stored `Other("g carb")`, and
    // carbs silently fail to convert (rolling up to zero) while every other
    // nutrient works. The source side was already singularized via
    // `measure.normalize()`, but normalizing it through the same function keeps
    // the two endpoints symmetric and order-independent.
    let mut input = input;
    let mut unit_a = input.unit().normalize();
    let unit_b = target.unit().normalize();

    // A pinch or dash is a fixed volumetric convention (see DASH_TO_TSP /
    // PINCH_TO_TSP), not a density — "1 pinch = 1/16 tsp" holds for salt, cayenne,
    // and everything else alike. Like the oz→g identity below, that makes it a
    // fact the engine owns rather than something a mapping has to supply, so
    // rescale into teaspoons and enter the graph at the tsp node. Doing it here
    // instead of seeding `pinch`/`dash` nodes in `make_graph` keeps them out of
    // every volume graph: `make_graph` never sees the source measure, so it would
    // have to add them unconditionally, polluting the graph viz and island
    // detection with nodes no mapping mentions.
    //
    // An explicit `pinch` node wins — if the caller mapped "1 pinch = 0.4 g" for
    // this product, that measured value beats the convention.
    let mut steps: Vec<ConversionStep> = Vec::new();
    if let Some(tsp) = tiny_volume_in_tsp(&unit_a)
        && !graph.node_indices().any(|i| graph[i] == unit_a)
    {
        steps.push(ConversionStep {
            from_unit: unit_a,
            to_unit: Unit::Teaspoon,
            factor: tsp,
        });
        input = Measure::new_with_upper(
            Unit::Teaspoon,
            input.value() * tsp,
            input.upper_value().map(|u| u * tsp),
        );
        unit_a = Unit::Teaspoon;
    }

    // Identity: once normalized, the measure may already sit in the target kind's
    // base unit (every mass unit normalizes to grams, and Weight targets grams).
    // That equivalence is a fixed, density-independent fact, so resolve it directly
    // instead of requiring both units to appear in the mapping graph. Without this,
    // a bare "8½ oz" with no product mapping can't reach Weight even though oz→g is
    // constant, and an empty graph fails even g→Weight.
    if unit_a == unit_b {
        // Round like the graph path below (the result is integer-rounded there),
        // so identity and multi-hop conversions agree to the unit. Apply the same
        // `upper > lower` suppression as the graph path so a ranged input whose
        // bounds round equal doesn't return a degenerate range here while the
        // graph path returns a point.
        let lo = input.value().round();
        let hi = input.upper_value().map(f64::round).filter(|&u| u > lo);
        let resolved = Measure::new_with_upper(unit_b, lo, hi);
        // `steps` is empty unless a pinch/dash rescale landed us on the target
        // node, in which case that one synthetic hop IS the whole path.
        return Some((resolved.denormalize(), steps));
    }

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
    let Some((_, path)) =
        petgraph::algo::astar(graph, n_a, |finish| finish == n_b, |_| 1.0, |_| 0.0)
    else {
        debug!("convert failed for {:?}", input);
        return None;
    };
    let mut factor_lo: f64 = 1.0;
    let mut factor_hi: f64 = 1.0;
    steps.reserve(path.len().saturating_sub(1));
    for x in 0..path.len() - 1 {
        let (n_from, n_to) = (*path.get(x)?, *path.get(x + 1)?);
        let edge = graph.find_edge(n_from, n_to)?;
        let weight = *graph.edge_weight(edge)?;
        steps.push(ConversionStep {
            from_unit: graph[n_from].clone(),
            to_unit: graph[n_to].clone(),
            // Report the lower bound: point edges have lower == upper, and the
            // only ranged edges (sub-recipe yields) never appear in ingredient
            // explain paths, so a single display factor stays faithful.
            factor: weight.lower,
        });
        factor_lo *= weight.lower;
        factor_hi *= weight.upper;
    }

    // Result range = input range × factor interval (all bounds positive):
    // lower = input_lo × factor_lo, upper = input_hi × factor_hi. Suppress an
    // upper that rounds equal to the lower so a point conversion of a point
    // amount never fabricates a range.
    let input_val = input.value();
    let input_upper = input.upper_value();
    let lower = (input_val * factor_lo).round();
    let upper = (input_upper.unwrap_or(input_val) * factor_hi).round();
    let result = Measure::new_with_upper(unit_b, lower, (upper > lower).then_some(upper));
    debug!("{:?} -> {:?} ({} hops)", input, result, path.len());
    Some((result.denormalize(), steps))
}

/// Unit node used by [`make_graph`] for a display unit string (e.g. `"cup"` →
/// [`Unit::Teaspoon`] after volume normalization).
pub fn mapping_graph_unit(unit: &str) -> Unit {
    Measure::new(unit, 1.0).normalize().unit().normalize()
}

/// [`MeasureKind`] for converting to a specific unit via user mappings.
pub fn mapping_target_kind(unit: &str) -> MeasureKind {
    MeasureKind::Other(mapping_graph_unit(unit).to_str().into_owned())
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
    fn mapping_target_kind_uses_normalized_graph_node() {
        assert_eq!(mapping_graph_unit("cup"), Unit::Teaspoon);
        assert_eq!(mapping_target_kind("cup").to_str(), "other:tsp");
    }

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
    fn test_convert_to_plural_nutrient_descriptor() {
        // Regression: a nutrient whose descriptor is plural ("g carbs") must still
        // convert. `make_graph` normalizes (singularizes) graph nodes, so the carb
        // edge becomes the node `Other("g carb")`. If the conversion *target* isn't
        // normalized the same way, the node lookup looks for `Other("g carbs")`,
        // misses, and carbs silently roll up to zero — even though "g protein" /
        // "g fiber" / "mg sodium" (none of which singularize) all work.
        let mappings = vec![(Measure::new("g", 100.0), Measure::new("g carbs", 67.7))];
        let measure = Measure::new("g", 27.0);
        let result = convert_measure_via_mappings(
            &measure,
            MeasureKind::Nutrient("g carbs".to_string()),
            &mappings,
        );
        assert!(
            result.is_some(),
            "plural nutrient descriptor failed to convert"
        );
        // 27 g * (67.7 / 100) = 18.279 -> 18
        assert_eq!(result.unwrap().value(), 18.0);
    }

    #[test]
    fn test_convert_to_plural_nutrient_descriptor_reversed_pair() {
        // `make_graph` adds both edge directions and normalizes both endpoints, so
        // the stored pair's order must not matter — the carb edge written
        // `67.7 g carbs = 100 g` converts identically to the `100 g = 67.7 g carbs`
        // form above. Guards that both conversion endpoints are normalized, not
        // just the target.
        let mappings = vec![(Measure::new("g carbs", 67.7), Measure::new("g", 100.0))];
        let measure = Measure::new("g", 27.0);
        let result = convert_measure_via_mappings(
            &measure,
            MeasureKind::Nutrient("g carbs".to_string()),
            &mappings,
        );
        assert_eq!(result.unwrap().value(), 18.0);
    }

    #[test]
    fn test_convert_to_singular_nutrient_descriptor() {
        // The descriptors that don't singularize always worked — keep them green so
        // the normalization fix can't regress the common case.
        let mappings = vec![(Measure::new("g", 100.0), Measure::new("g fiber", 10.0))];
        let measure = Measure::new("g", 50.0);
        let result = convert_measure_via_mappings(
            &measure,
            MeasureKind::Nutrient("g fiber".to_string()),
            &mappings,
        );
        assert_eq!(result.unwrap().value(), 5.0);
    }

    #[test]
    fn test_print_graph() {
        let mappings = vec![(Measure::new("cup", 1.0), Measure::new("g", 120.0))];

        let graph = make_graph(&mappings);
        let dot = print_graph(&graph);

        assert!(dot.contains("digraph"));
    }

    #[test]
    fn test_print_graph_escapes_quotes_in_labels() {
        // A unit can contain a double quote (the inch symbol, e.g. `1" cubes`).
        // Unescaped, it would terminate the DOT label early and yield invalid
        // DOT that the renderer rejects. It must be emitted as `\"`.
        let mappings = vec![(Measure::new("1\" cube", 1.0), Measure::new("g", 30.0))];
        let dot = print_graph(&make_graph(&mappings));

        assert!(
            dot.contains("label = \"1\\\" cube\""),
            "the inch-quote must be backslash-escaped in the label: {dot}"
        );
        // And the raw, unescaped `1" cube"` sequence must not appear.
        assert!(
            !dot.contains("\"1\" cube\""),
            "must not emit an unescaped quote: {dot}"
        );
    }

    #[test]
    fn test_print_graph_readability() {
        // 1 cup = 120 g. A cup normalizes to tsp (48 tsp/cup), so the graph holds
        // tsp, g, and the synthesized ml bridge.
        let mappings = vec![(Measure::new("cup", 1.0), Measure::new("g", 120.0))];
        let dot = print_graph(&make_graph(&mappings));

        // Nodes are tagged with their MeasureKind so the host can theme them.
        assert!(
            dot.contains("class = \"volume\""),
            "tsp/ml should be volume: {dot}"
        );
        assert!(
            dot.contains("class = \"weight\""),
            "g should be weight: {dot}"
        );

        // Opposing edges collapse into one bidirectional edge per pair...
        assert!(
            dot.contains("dir = \"both\""),
            "edges should be bidirectional: {dot}"
        );
        assert_eq!(
            dot.matches("->").count(),
            2,
            "two pairs (tsp↔g, tsp↔ml) → exactly two edges, not four: {dot}"
        );

        // ...the synthesized tsp↔ml bridge is dashed...
        assert!(
            dot.contains("style = \"dashed\""),
            "bridge should be dashed: {dot}"
        );

        // ...and factors are rounded to 3 sig figs: the tsp↔ml bridge constant
        // (4.92892159375) renders as "4.93", not its full-precision tail.
        assert!(
            dot.contains("4.93"),
            "bridge factor should round to 4.93: {dot}"
        );
        assert!(
            !dot.contains("4.928921"),
            "factors should be rounded, not full-precision: {dot}"
        );
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
    fn mass_amount_converts_to_weight_without_mappings() {
        // oz and g are both mass — convertible by a fixed constant, with no
        // density mapping. 8.5 oz * 28.3495 ≈ 240.97 g.
        let oz = convert_measure_via_mappings(&Measure::new("oz", 8.5), MeasureKind::Weight, &[]);
        assert_eq!(oz.map(|m| m.value().round()), Some(241.0));

        // An already-gram measure resolves trivially even on an empty graph.
        let g = convert_measure_via_mappings(&Measure::new("g", 100.0), MeasureKind::Weight, &[]);
        assert_eq!(g.map(|m| m.value()), Some(100.0));

        // lb too: 1 lb = 16 oz = 453.59 g.
        let lb = convert_measure_via_mappings(&Measure::new("lb", 1.0), MeasureKind::Weight, &[]);
        assert_eq!(lb.map(|m| m.value().round()), Some(454.0));

        // Volume still needs a density — no identity, no mapping → not convertible.
        let cup = convert_measure_via_mappings(&Measure::new("cup", 1.0), MeasureKind::Weight, &[]);
        assert!(cup.is_none());
    }

    #[test]
    fn test_convert_measure_no_source_node() {
        // A genuinely-unknown unit — not a pinch/dash, which the engine bridges
        // into teaspoons on its own (see `pinch_and_dash_bridge_to_tsp`).
        let measure = Measure::new("clove", 1.0);
        let mappings = vec![(Measure::new("cup", 1.0), Measure::new("g", 120.0))];

        let result = convert_measure_via_mappings(&measure, MeasureKind::Weight, &mappings);

        assert!(result.is_none());
    }

    #[test]
    fn pinch_and_dash_bridge_to_tsp() {
        // A pinch/dash is a fixed fraction of a teaspoon, so a product that has
        // any volume→weight density resolves them with no pinch-specific mapping.
        // 1 cup = 120 g ⇒ 1 tsp = 2.5 g ⇒ 1 pinch (1/16 tsp) = 0.15625 g → 0 g
        // rounded, so use a denser mapping to keep the assertion off the rounding
        // floor: 1 cup = 480 g ⇒ 1 tsp = 10 g.
        let mappings = vec![(Measure::new("cup", 1.0), Measure::new("g", 480.0))];

        // 1 pinch = 1/16 tsp = 0.625 g → 1 g.
        let pinch = convert_measure_via_mappings(
            &Measure::new("pinch", 1.0),
            MeasureKind::Weight,
            &mappings,
        );
        assert_eq!(pinch.map(|m| m.value()), Some(1.0));

        // 16 pinches = 1 tsp = 10 g exactly.
        let pinches = convert_measure_via_mappings(
            &Measure::new("pinches", 16.0),
            MeasureKind::Weight,
            &mappings,
        );
        assert_eq!(pinches.map(|m| m.value()), Some(10.0));

        // A dash is twice a pinch: 8 dashes = 1 tsp = 10 g.
        let dashes = convert_measure_via_mappings(
            &Measure::new("dashes", 8.0),
            MeasureKind::Weight,
            &mappings,
        );
        assert_eq!(dashes.map(|m| m.value()), Some(10.0));
    }

    #[test]
    fn pinch_bridge_shows_in_the_explain_path() {
        // The synthetic hop must appear in the explained steps, so a costing
        // explanation reads "pinch —×0.0625→ tsp —×10→ g" rather than starting
        // mid-path at teaspoons.
        let graph = make_graph(&[(Measure::new("cup", 1.0), Measure::new("g", 480.0))]);
        let (_, steps) = convert_measure_with_graph_explained(
            &Measure::new("pinch", 1.0),
            MeasureKind::Weight,
            &graph,
        )
        .unwrap();

        assert_eq!(
            steps.first().map(|s| s.from_unit.clone()),
            Some(pinch_unit())
        );
        assert_eq!(
            steps.first().map(|s| s.to_unit.clone()),
            Some(Unit::Teaspoon)
        );
        assert_eq!(steps.first().map(|s| s.factor), Some(1.0 / 16.0));
        assert_eq!(steps.last().map(|s| s.to_unit.clone()), Some(Unit::Gram));
    }

    #[test]
    fn explicit_pinch_mapping_beats_the_convention() {
        // "1 pinch = 5 g" for this product is a measurement; the 1/16-tsp
        // convention is a default. The measurement wins, and no synthetic hop is
        // emitted.
        let graph = make_graph(&[
            (Measure::new("cup", 1.0), Measure::new("g", 480.0)),
            (Measure::new("pinch", 1.0), Measure::new("g", 5.0)),
        ]);
        let (converted, steps) = convert_measure_with_graph_explained(
            &Measure::new("pinch", 2.0),
            MeasureKind::Weight,
            &graph,
        )
        .unwrap();

        assert_eq!(converted.value(), 10.0);
        assert_eq!(steps.len(), 1);
        assert_eq!(
            steps.first().map(|s| s.from_unit.clone()),
            Some(pinch_unit())
        );
    }

    #[test]
    fn pinch_converts_to_volume_and_carries_a_range() {
        let mappings = vec![(Measure::new("cup", 1.0), Measure::new("g", 480.0))];

        // Volume targets ml, reached via the seeded tsp↔ml bridge:
        // 16 pinches = 1 tsp = 4.92892 ml → 5 ml.
        let ml = convert_measure_via_mappings(
            &Measure::new("pinch", 16.0),
            MeasureKind::Volume,
            &mappings,
        );
        assert_eq!(ml.map(|m| m.value()), Some(5.0));

        // A ranged amount ("1–2 pinches") scales both bounds.
        let ranged = convert_measure_via_mappings(
            &Measure::with_range("pinch", 16.0, 32.0),
            MeasureKind::Weight,
            &mappings,
        );
        let ranged = ranged.unwrap();
        assert_eq!(ranged.value(), 10.0);
        assert_eq!(ranged.upper_value(), Some(20.0));
    }

    /// The normalized graph unit for a pinch, spelled once.
    fn pinch_unit() -> Unit {
        Unit::Other("pinch".to_string())
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
    fn test_ranged_edge_propagates_to_result() {
        // A sub-recipe yield mapping "1 batch = $6–8" (the batch holds a ranged
        // ingredient). Converting 2 batches to Money must produce $12–16 — both
        // bounds ride the interval factor, even though the input amount is a point.
        let mappings = vec![(
            Measure::new("batch", 1.0),
            Measure::with_range("dollar", 6.0, 8.0),
        )];
        let graph = make_graph(&mappings);
        let r = convert_measure_with_graph(&Measure::new("batch", 2.0), MeasureKind::Money, &graph)
            .unwrap();
        assert_eq!(r.value(), 12.0);
        assert_eq!(r.upper_value(), Some(16.0));
    }

    #[test]
    fn test_ranged_edge_and_ranged_input_compose() {
        // "1 batch = $6–8", convert "2–3 batches" → [2×6, 3×8] = $12–24.
        let mappings = vec![(
            Measure::new("batch", 1.0),
            Measure::with_range("dollar", 6.0, 8.0),
        )];
        let graph = make_graph(&mappings);
        let r = convert_measure_with_graph(
            &Measure::with_range("batch", 2.0, 3.0),
            MeasureKind::Money,
            &graph,
        )
        .unwrap();
        assert_eq!(r.value(), 12.0);
        assert_eq!(r.upper_value(), Some(24.0));
    }

    #[test]
    fn test_ranged_edge_inverts_correctly() {
        // Traversing a ranged edge backward uses interval reciprocal with a bound
        // flip: 24 g back through "1 widget = 6–8 g" → [24/8, 24/6] = 3–4 widgets.
        let mappings = vec![(
            Measure::new("widget", 1.0),
            Measure::with_range("g", 6.0, 8.0),
        )];
        let graph = make_graph(&mappings);
        let r = convert_measure_with_graph(
            &Measure::new("g", 24.0),
            MeasureKind::Other("widget".to_string()),
            &graph,
        )
        .unwrap();
        assert_eq!(r.value(), 3.0);
        assert_eq!(r.upper_value(), Some(4.0));
    }

    #[test]
    fn test_point_conversion_never_fabricates_range() {
        // A point amount through a point mapping stays a point — no spurious upper.
        let mappings = vec![(Measure::new("cup", 1.0), Measure::new("g", 120.0))];
        let graph = make_graph(&mappings);
        let r = convert_measure_with_graph(&Measure::new("cup", 2.0), MeasureKind::Weight, &graph)
            .unwrap();
        assert_eq!(r.value(), 240.0);
        assert_eq!(r.upper_value(), None);
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

    #[test]
    fn test_explained_path_factors_multiply_to_conversion() {
        // widget -> g -> dollar: two hops whose step factors must multiply to
        // the overall factor, with units reported in traversal order.
        let mappings = vec![
            (Measure::new("widget", 1.0), Measure::new("g", 200.0)),
            (Measure::new("g", 100.0), Measure::new("dollar", 2.0)),
        ];
        let graph = make_graph(&mappings);

        let (result, steps) = convert_measure_with_graph_explained(
            &Measure::new("widget", 3.0),
            MeasureKind::Money,
            &graph,
        )
        .unwrap();

        assert_eq!(result.value(), 12.0); // 3 widget = 600 g = $12
        assert_eq!(steps.len(), 2);
        // Steps report the NORMALIZED graph nodes: money normalizes to cents.
        assert_eq!(steps[0].from_unit.to_str(), "widget");
        assert_eq!(steps[0].to_unit.to_str(), "g");
        assert_eq!(steps[1].to_unit.to_str(), "cent");
        // 200 g/widget × 2 cent/g = 400 cents/widget; 3 widget = 1200 cents = $12.
        let product: f64 = steps.iter().map(|s| s.factor).product();
        assert!((product - 400.0).abs() < 1e-9);
    }

    #[test]
    fn test_explained_same_node_is_empty_path() {
        let mappings = vec![(Measure::new("widget", 1.0), Measure::new("g", 200.0))];
        let graph = make_graph(&mappings);

        let (result, steps) = convert_measure_with_graph_explained(
            &Measure::new("g", 50.0),
            MeasureKind::Weight,
            &graph,
        )
        .unwrap();

        assert_eq!(result.value(), 50.0);
        assert!(steps.is_empty());
    }
}
