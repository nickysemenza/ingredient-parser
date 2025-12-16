use crate::unit::singular;
use crate::unit::{kind::MeasureKind, Unit};
use crate::util::{num_without_zeroes, round_to_int, truncate_3_decimals};
use crate::{IngredientError, IngredientResult};
use petgraph::Graph;
use serde::{de::Deserializer, ser::Serializer, Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use tracing::{debug, info};

pub type MeasureGraph = Graph<Unit, f64>;

pub fn make_graph(mappings: Vec<(Measure, Measure)>) -> MeasureGraph {
    let mut g = Graph::<Unit, f64>::new();

    for (mut m_a, mut m_b) in mappings.into_iter() {
        m_a = m_a.normalize();
        m_b = m_b.normalize();
        let n_a = g
            .node_indices()
            .find(|i| g[*i] == m_a.unit)
            .unwrap_or_else(|| g.add_node(m_a.unit.clone().normalize()));
        let n_b = g
            .node_indices()
            .find(|i| g[*i] == m_b.unit)
            .unwrap_or_else(|| g.add_node(m_b.unit.clone().normalize()));

        let a_to_b_weight = truncate_3_decimals(m_b.value / m_a.value);

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
            g.add_edge(n_b, n_a, truncate_3_decimals(m_a.value / m_b.value));
        }
    }
    g
}
pub fn print_graph(g: MeasureGraph) -> String {
    format!("{}", petgraph::dot::Dot::new(&g))
}

#[derive(Clone, PartialEq, PartialOrd, Debug, Serialize, Deserialize)]
pub struct Measure {
    #[serde(
        serialize_with = "serialize_unit",
        deserialize_with = "deserialize_unit"
    )]
    unit: Unit,
    value: f64,
    upper_value: Option<f64>,
}

/// Serialize Unit as its canonical string form (e.g., "cup", "g", "$")
fn serialize_unit<S: Serializer>(unit: &Unit, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&unit.to_str())
}

/// Deserialize Unit from a string
fn deserialize_unit<'de, D: Deserializer<'de>>(d: D) -> Result<Unit, D::Error> {
    let s = String::deserialize(d)?;
    Ok(Unit::from_str(&s).unwrap_or(Unit::Other(singular(&s))))
}

// Multiplication factors for unit conversions
const TSP_TO_TBSP: f64 = 3.0;
const TSP_TO_FL_OZ: f64 = 2.0;
const G_TO_K: f64 = 1000.0;
const CUP_TO_QUART: f64 = 4.0;
const TSP_TO_CUP: f64 = 48.0;
const GRAM_TO_OZ: f64 = 28.3495;
const OZ_TO_LB: f64 = 16.0;
const CENTS_TO_DOLLAR: f64 = 100.0;
const SEC_TO_MIN: f64 = 60.0;
const SEC_TO_HOUR: f64 = 3600.0;
const SEC_TO_DAY: f64 = 86400.0;

/// Normalization rule: convert `from` unit to `to_base` unit by multiplying by `factor`
struct NormalizationRule {
    from: Unit,
    to_base: Unit,
    factor: f64,
}

/// Rules for normalizing units to their base units
static NORMALIZATION_RULES: &[NormalizationRule] = &[
    // Weight: normalize to grams
    NormalizationRule {
        from: Unit::Kilogram,
        to_base: Unit::Gram,
        factor: G_TO_K,
    },
    NormalizationRule {
        from: Unit::Ounce,
        to_base: Unit::Gram,
        factor: GRAM_TO_OZ,
    },
    NormalizationRule {
        from: Unit::Pound,
        to_base: Unit::Gram,
        factor: GRAM_TO_OZ * OZ_TO_LB,
    },
    // Volume: normalize to teaspoons (or milliliters)
    NormalizationRule {
        from: Unit::Liter,
        to_base: Unit::Milliliter,
        factor: G_TO_K,
    },
    NormalizationRule {
        from: Unit::Tablespoon,
        to_base: Unit::Teaspoon,
        factor: TSP_TO_TBSP,
    },
    NormalizationRule {
        from: Unit::Cup,
        to_base: Unit::Teaspoon,
        factor: TSP_TO_CUP,
    },
    NormalizationRule {
        from: Unit::Quart,
        to_base: Unit::Teaspoon,
        factor: CUP_TO_QUART * TSP_TO_CUP,
    },
    NormalizationRule {
        from: Unit::FluidOunce,
        to_base: Unit::Teaspoon,
        factor: TSP_TO_FL_OZ,
    },
    // Money: normalize to cents
    NormalizationRule {
        from: Unit::Dollar,
        to_base: Unit::Cent,
        factor: CENTS_TO_DOLLAR,
    },
    // Time: normalize to seconds
    NormalizationRule {
        from: Unit::Minute,
        to_base: Unit::Second,
        factor: SEC_TO_MIN,
    },
    NormalizationRule {
        from: Unit::Hour,
        to_base: Unit::Second,
        factor: SEC_TO_HOUR,
    },
    NormalizationRule {
        from: Unit::Day,
        to_base: Unit::Second,
        factor: SEC_TO_DAY,
    },
];

/// Find the normalization rule for a given unit
fn find_normalization_rule(unit: &Unit) -> Option<&'static NormalizationRule> {
    NORMALIZATION_RULES.iter().find(|rule| &rule.from == unit)
}

/// Known nutrient unit prefixes (mass/energy units used for nutrients)
static NUTRIENT_UNIT_PREFIXES: &[&str] = &["g", "mg", "ug", "µg", "mcg", "kcal", "iu"];

/// Known nutrient names that follow the unit prefix
static NUTRIENT_NAMES: &[&str] = &[
    "protein",
    "fat",
    "carbs",
    "fiber",
    "calcium",
    "iron",
    "magnesium",
    "potassium",
    "sodium",
    "zinc",
    "selenium",
    "cholesterol",
    "saturated_fat",
    "vitamin_a",
    "vitamin_b6",
    "vitamin_b12",
    "vitamin_c",
    "vitamin_d",
    "vitamin_e",
    "vitamin_k",
    "folate",
];

/// Check if a unit string represents a nutrient (e.g., "g protein", "mg sodium")
fn is_nutrient_unit(s: &str) -> bool {
    let parts: Vec<&str> = s.split_whitespace().collect();
    if parts.len() != 2 {
        return false;
    }
    let prefix = parts[0].to_lowercase();
    let name = parts[1].to_lowercase();
    NUTRIENT_UNIT_PREFIXES.contains(&prefix.as_str()) && NUTRIENT_NAMES.contains(&name.as_str())
}

impl Measure {
    pub(crate) fn new_with_upper(unit: Unit, value: f64, upper_value: Option<f64>) -> Measure {
        Measure {
            unit,
            value,
            upper_value,
        }
    }
    pub fn unit(&self) -> Unit {
        self.unit.clone()
    }
    pub fn values(&self) -> (f64, Option<f64>, String) {
        (self.value, self.upper_value, self.unit_as_string())
    }
    /// Normalize this measure to its base unit
    ///
    /// Converts units like cups to teaspoons, kg to grams, etc.
    /// Uses the NORMALIZATION_RULES table for conversion factors.
    pub(crate) fn normalize(&self) -> Measure {
        // Handle custom units - normalize the unit name (singularize)
        if let Unit::Other(x) = &self.unit {
            return Measure::new_with_upper(Unit::Other(singular(x)), self.value, self.upper_value);
        }

        // Look up conversion rule in the table
        if let Some(rule) = find_normalization_rule(&self.unit) {
            return Measure {
                unit: rule.to_base.clone(),
                value: self.value * rule.factor,
                upper_value: self.upper_value.map(|x| x * rule.factor),
            };
        }

        // Unit is already a base unit, return as-is
        self.clone()
    }
    pub fn add(&self, b: Measure) -> IngredientResult<Measure> {
        info!("adding {:?} to {:?}", self, b);

        // Get kinds with proper error handling
        let b_kind = b.kind()?;
        let self_kind = self.kind()?;

        if let MeasureKind::Other(_) = b_kind {
            return Ok(self.clone());
        }

        if self_kind != b_kind {
            return Err(IngredientError::MeasureError {
                operation: "add".to_string(),
                reason: format!(
                    "Cannot add measures of different kinds: {self_kind:?} and {b_kind:?}"
                ),
            });
        }
        let left = self.normalize();
        let right = b.normalize();

        Ok(Measure {
            unit: left.unit.clone(),
            value: left.value + right.value,
            upper_value: match (left.upper_value, right.upper_value) {
                (Some(a), Some(b)) => Some(a + b),
                (None, None) => None,
                (None, Some(b)) => Some(left.value + b),
                (Some(a), None) => Some(a + right.value),
            },
        })
    }
    /// Create a new measure from a unit string and value
    ///
    /// # Arguments
    /// * `unit` - The unit string (e.g., "cups", "grams")
    /// * `value` - The numeric value
    ///
    /// # Example
    /// ```
    /// use ingredient::unit::Measure;
    /// let m = Measure::new("cups", 2.0);
    /// ```
    pub fn new(unit: &str, value: f64) -> Measure {
        Measure::from_parts(unit, value, None)
    }

    /// Create a new measure with a range (lower to upper value)
    ///
    /// # Arguments
    /// * `unit` - The unit string (e.g., "cups", "grams")
    /// * `lower` - The lower bound of the range
    /// * `upper` - The upper bound of the range
    ///
    /// # Example
    /// ```
    /// use ingredient::unit::Measure;
    /// let m = Measure::with_range("cups", 2.0, 3.0);
    /// ```
    pub fn with_range(unit: &str, lower: f64, upper: f64) -> Measure {
        Measure::from_parts(unit, lower, Some(upper))
    }

    /// Create a measure from parts (core implementation)
    ///
    /// This is the low-level constructor used by `new` and `with_range`.
    pub fn from_parts(unit: &str, value: f64, upper_value: Option<f64>) -> Measure {
        let normalized_unit = singular(unit);
        let unit = Unit::from_str(normalized_unit.as_ref()).unwrap_or(Unit::Other(normalized_unit));

        Measure {
            unit,
            value,
            upper_value,
        }
    }
    /// Get the kind/category of this measurement (weight, volume, time, etc.)
    ///
    /// This uses direct mapping without recursion for better performance
    /// and to avoid potential stack overflow on malformed data.
    pub fn kind(&self) -> IngredientResult<MeasureKind> {
        Ok(match &self.unit {
            // Weight units
            Unit::Gram | Unit::Kilogram | Unit::Ounce | Unit::Pound => MeasureKind::Weight,

            // Volume units
            Unit::Milliliter
            | Unit::Liter
            | Unit::Teaspoon
            | Unit::Tablespoon
            | Unit::Cup
            | Unit::Quart
            | Unit::FluidOunce => MeasureKind::Volume,

            // Money units
            Unit::Cent | Unit::Dollar => MeasureKind::Money,

            // Time units
            Unit::Second | Unit::Minute | Unit::Hour | Unit::Day => MeasureKind::Time,

            // Temperature units
            Unit::Fahrenheit | Unit::Celcius => MeasureKind::Temperature,

            // Energy units
            Unit::KCal => MeasureKind::Calories,

            // Length units
            Unit::Inch => MeasureKind::Length,

            // Other/custom units
            Unit::Whole => MeasureKind::Other("whole".to_string()),
            Unit::Other(s) => {
                // Check if this is a nutrient unit pattern like "g protein", "mg sodium", "ug vitamin_b12"
                if is_nutrient_unit(s) {
                    MeasureKind::Nutrient(s.clone())
                } else {
                    MeasureKind::Other(s.clone())
                }
            }
        })
    }

    pub fn denormalize(&self) -> Measure {
        let (u, f) = match &self.unit {
            Unit::Gram => (Unit::Gram, 1.0),
            Unit::Milliliter => (Unit::Milliliter, 1.0),
            Unit::Teaspoon => match self.value {
                // only for these measurements to we convert to the best fit, others stay bare due to the nature of the values
                m if { m < 3.0 } => (Unit::Teaspoon, 1.0),
                m if { m < 12.0 } => (Unit::Tablespoon, TSP_TO_TBSP),
                m if { m < CUP_TO_QUART * TSP_TO_CUP } => (Unit::Cup, TSP_TO_CUP),
                _ => (Unit::Quart, CUP_TO_QUART * TSP_TO_CUP),
            },
            Unit::Cent => (Unit::Dollar, CENTS_TO_DOLLAR),
            Unit::KCal => (Unit::KCal, 1.0),
            Unit::Second => match self.value {
                // only for these measurements to we convert to the best fit, others stay bare due to the nature of the values
                m if { m < SEC_TO_MIN } => (Unit::Second, 1.0),
                m if { m < SEC_TO_HOUR } => (Unit::Minute, SEC_TO_MIN),
                m if { m < SEC_TO_DAY } => (Unit::Hour, SEC_TO_HOUR),
                _ => (Unit::Day, SEC_TO_DAY),
            },
            Unit::Inch => (Unit::Inch, 1.0),
            Unit::Other(o) => (Unit::Other(o.clone()), 1.0),
            Unit::Kilogram
            | Unit::Liter
            | Unit::Tablespoon
            | Unit::Cup
            | Unit::Quart
            | Unit::FluidOunce
            | Unit::Ounce
            | Unit::Pound
            | Unit::Dollar
            | Unit::Fahrenheit
            | Unit::Celcius // todo: convert to farhenheit?
            | Unit::Whole
            | Unit::Minute
            | Unit::Hour
            | Unit::Day => return self.clone(),
        };
        Measure {
            unit: u,
            value: self.value / f,
            upper_value: self.upper_value.map(|x| x / f),
        }
    }

    #[tracing::instrument]
    pub fn convert_measure_via_mappings(
        &self,
        target: MeasureKind,
        mappings: Vec<(Measure, Measure)>,
    ) -> Option<Measure> {
        let g = make_graph(mappings);
        let input = self.normalize();
        let unit_a = input.unit.clone();
        let unit_b = target.unit();

        let n_a = g.node_indices().find(|i| g[*i] == unit_a)?;
        let n_b = g.node_indices().find(|i| g[*i] == unit_b)?;

        debug!("calculating {:?} to {:?}", n_a, n_b);
        if !petgraph::algo::has_path_connecting(&g, n_a, n_b, None) {
            debug!("convert failed for {:?}", input);
            return None;
        };

        let steps =
            petgraph::algo::astar(&g, n_a, |finish| finish == n_b, |e| *e.weight(), |_| 0.0)?.1;
        let mut factor: f64 = 1.0;
        for x in 0..steps.len() - 1 {
            let edge = g.find_edge(*steps.get(x)?, *steps.get(x + 1)?)?;
            factor *= g.edge_weight(edge)?;
        }

        let result = Measure::new_with_upper(
            unit_b,
            round_to_int(input.value * factor),
            input.upper_value.map(|x| round_to_int(x * factor)),
        );
        debug!("{:?} -> {:?} ({} hops)", input, result, steps.len());
        Some(result.denormalize())
    }
    fn unit_as_string(&self) -> String {
        let mut s = singular(&self.unit().to_str());
        if (self.unit() == Unit::Cup || self.unit() == Unit::Minute)
            && (self.value > 1.0 || self.upper_value.unwrap_or_default() > 1.0)
        {
            s.push('s');
        }
        s
    }
}

impl fmt::Display for Measure {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let measure = self.denormalize();
        if let Some(u) = measure.upper_value {
            if u != 0.0 {
                if measure.value == 0.0 {
                    // "up to X" case - just show the upper bound
                    write!(f, "{} {}", num_without_zeroes(u), self.unit_as_string())
                } else {
                    // Normal range "X - Y"
                    write!(
                        f,
                        "{} - {} {}",
                        num_without_zeroes(measure.value),
                        num_without_zeroes(u),
                        self.unit_as_string()
                    )
                }
            } else {
                write!(
                    f,
                    "{} {}",
                    num_without_zeroes(measure.value),
                    self.unit_as_string()
                )
            }
        } else {
            write!(
                f,
                "{} {}",
                num_without_zeroes(measure.value),
                self.unit_as_string()
            )
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_measure() {
        let m1 = Measure::new("tbsp", 16.0);
        assert_eq!(
            m1.normalize(),
            Measure::new_with_upper(Unit::Teaspoon, 48.0, None)
        );
        assert_eq!(m1.normalize(), Measure::new("cup", 1.0).normalize());
        assert_eq!(
            Measure::new("grams", 25.2).denormalize(),
            Measure::new("g", 25.2)
        );
        assert_eq!(
            Measure::new("grams", 2500.2).denormalize(),
            Measure::new("g", 2500.2)
        );
    }

    #[test]
    fn test_singular_plural() {
        assert_eq!(Measure::new("cup", 1.0).unit_as_string(), "cup");
        assert_eq!(Measure::new("cup", 2.0).unit_as_string(), "cups");
        assert_eq!(Measure::new("grams", 3.0).unit_as_string(), "g");
    }

    #[test]
    fn test_display_range() {
        // Normal range displays as "X - Y unit"
        assert_eq!(
            Measure::with_range("days", 1.0, 3.0).to_string(),
            "1 - 3 day"
        );

        // "up to X" (0 to X) displays as just "X unit" without the "0 -" prefix
        assert_eq!(Measure::with_range("days", 0.0, 3.0).to_string(), "3 day");

        // Single value displays normally
        assert_eq!(Measure::new("hours", 2.0).to_string(), "2 hour");
    }

    #[test]
    fn test_is_nutrient_unit() {
        // Valid nutrient units
        assert!(is_nutrient_unit("g protein"));
        assert!(is_nutrient_unit("mg sodium"));
        assert!(is_nutrient_unit("ug vitamin_b12"));
        assert!(is_nutrient_unit("G PROTEIN")); // case insensitive
        assert!(is_nutrient_unit("MG Calcium"));
        assert!(is_nutrient_unit("kcal fat")); // kcal is a valid prefix for nutrients

        // Invalid - not nutrient patterns
        assert!(!is_nutrient_unit("g")); // no nutrient name
        assert!(!is_nutrient_unit("protein")); // no unit prefix
        assert!(!is_nutrient_unit("cups")); // regular unit
        assert!(!is_nutrient_unit("g unknown")); // unknown nutrient name
        assert!(!is_nutrient_unit("xyz protein")); // unknown unit prefix
        assert!(!is_nutrient_unit("g protein extra")); // too many parts
    }

    #[test]
    fn test_measure_kind_nutrients() {
        // Nutrient units should return MeasureKind::Nutrient
        let m_protein = Measure::new("g protein", 12.5);
        assert!(matches!(
            m_protein.kind().unwrap(),
            MeasureKind::Nutrient(_)
        ));
        assert_eq!(
            m_protein.kind().unwrap(),
            MeasureKind::Nutrient("g protein".to_string())
        );

        let m_sodium = Measure::new("mg sodium", 500.0);
        assert_eq!(
            m_sodium.kind().unwrap(),
            MeasureKind::Nutrient("mg sodium".to_string())
        );

        let m_b12 = Measure::new("ug vitamin_b12", 2.4);
        assert_eq!(
            m_b12.kind().unwrap(),
            MeasureKind::Nutrient("ug vitamin_b12".to_string())
        );

        // Non-nutrient "Other" units should still be Other
        let m_whole = Measure::new("whole", 1.0);
        assert!(matches!(m_whole.kind().unwrap(), MeasureKind::Other(_)));

        let m_slice = Measure::new("slice", 2.0);
        assert_eq!(
            m_slice.kind().unwrap(),
            MeasureKind::Other("slice".to_string())
        );
    }

    #[test]
    fn test_measure_kind_is_scalable() {
        // Scalable kinds (should increase when doubling a recipe)
        assert!(MeasureKind::Weight.is_scalable());
        assert!(MeasureKind::Volume.is_scalable());
        assert!(MeasureKind::Other("pinch".to_string()).is_scalable());
        assert!(MeasureKind::Other("clove".to_string()).is_scalable());

        // Non-scalable kinds (should stay constant when scaling)
        assert!(!MeasureKind::Time.is_scalable());
        assert!(!MeasureKind::Temperature.is_scalable());
        assert!(!MeasureKind::Calories.is_scalable());
        assert!(!MeasureKind::Money.is_scalable());
        assert!(!MeasureKind::Length.is_scalable());
        assert!(!MeasureKind::Nutrient("g protein".to_string()).is_scalable());
    }
}
