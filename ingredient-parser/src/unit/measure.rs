use crate::unit::singular;
use crate::unit::{kind::MeasureKind, Unit};
use crate::util::num_without_zeroes;
use crate::{IngredientError, IngredientResult};
use serde::{de::Deserializer, ser::Serializer, Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use tracing::info;

// Re-export conversion types and functions for backward compatibility
pub use super::conversion::{convert_measure_via_mappings, make_graph, print_graph, MeasureGraph};

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

    /// Convert this measure to a target kind using user-provided mappings.
    ///
    /// This is a convenience wrapper around `convert_measure_via_mappings`.
    pub fn convert_measure_via_mappings(
        &self,
        target: MeasureKind,
        mappings: Vec<(Measure, Measure)>,
    ) -> Option<Measure> {
        convert_measure_via_mappings(self, target, mappings)
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
    use rstest::rstest;

    // ============================================================================
    // Measure Normalization Tests
    // ============================================================================

    #[test]
    fn test_measure_normalize() {
        let m1 = Measure::new("tbsp", 16.0);
        assert_eq!(
            m1.normalize(),
            Measure::new_with_upper(Unit::Teaspoon, 48.0, None)
        );
        assert_eq!(m1.normalize(), Measure::new("cup", 1.0).normalize());
    }

    #[rstest]
    #[case::grams_small("grams", 25.2, "g", 25.2)]
    #[case::grams_large("grams", 2500.2, "g", 2500.2)]
    #[case::inch("inch", 5.0, "\"", 5.0)]
    fn test_measure_denormalize(
        #[case] input_unit: &str,
        #[case] input_value: f64,
        #[case] expected_unit: &str,
        #[case] expected_value: f64,
    ) {
        let m = Measure::new(input_unit, input_value);
        let d = m.denormalize();
        assert_eq!(d.unit().to_str(), expected_unit);
        assert_eq!(d.values().0, expected_value);
    }

    // ============================================================================
    // Teaspoon and Second Denormalization (Unit-based)
    // ============================================================================

    #[rstest]
    #[case::small_tsp(2.0, Unit::Teaspoon)]
    #[case::tablespoon(6.0, Unit::Tablespoon)]
    #[case::cup(48.0, Unit::Cup)]
    #[case::quart(200.0, Unit::Quart)]
    fn test_teaspoon_denormalize_unit(#[case] value: f64, #[case] expected: Unit) {
        let m = Measure::new_with_upper(Unit::Teaspoon, value, None);
        assert_eq!(m.denormalize().unit(), expected);
    }

    #[rstest]
    #[case::small_sec(30.0, Unit::Second)]
    #[case::minute(120.0, Unit::Minute)]
    #[case::hour(7200.0, Unit::Hour)]
    #[case::day(90000.0, Unit::Day)]
    fn test_second_denormalize_unit(#[case] value: f64, #[case] expected: Unit) {
        let m = Measure::new_with_upper(Unit::Second, value, None);
        assert_eq!(m.denormalize().unit(), expected);
    }

    // ============================================================================
    // Singular/Plural Tests
    // ============================================================================

    #[rstest]
    #[case::cup_singular("cup", 1.0, "cup")]
    #[case::cup_plural("cup", 2.0, "cups")]
    #[case::grams_no_plural("grams", 3.0, "g")]
    fn test_singular_plural(#[case] unit: &str, #[case] value: f64, #[case] expected: &str) {
        assert_eq!(Measure::new(unit, value).unit_as_string(), expected);
    }

    // ============================================================================
    // Nutrient Unit Tests
    // ============================================================================

    #[rstest]
    #[case::g_protein("g protein", true)]
    #[case::mg_sodium("mg sodium", true)]
    #[case::ug_b12("ug vitamin_b12", true)]
    #[case::case_insensitive("G PROTEIN", true)]
    #[case::mixed_case("MG Calcium", true)]
    #[case::kcal_fat("kcal fat", true)]
    #[case::no_nutrient("g", false)]
    #[case::no_prefix("protein", false)]
    #[case::regular_unit("cups", false)]
    #[case::unknown_nutrient("g unknown", false)]
    #[case::unknown_prefix("xyz protein", false)]
    #[case::too_many_parts("g protein extra", false)]
    fn test_is_nutrient_unit(#[case] input: &str, #[case] expected: bool) {
        assert_eq!(is_nutrient_unit(input), expected);
    }

    #[rstest]
    #[case::protein("g protein", 12.5, "g protein")]
    #[case::sodium("mg sodium", 500.0, "mg sodium")]
    #[case::b12("ug vitamin_b12", 2.4, "ug vitamin_b12")]
    fn test_measure_kind_nutrients(
        #[case] unit: &str,
        #[case] value: f64,
        #[case] expected_nutrient: &str,
    ) {
        let m = Measure::new(unit, value);
        assert_eq!(
            m.kind().unwrap(),
            MeasureKind::Nutrient(expected_nutrient.to_string())
        );
    }

    #[test]
    fn test_measure_kind_other_units() {
        let m_whole = Measure::new("whole", 1.0);
        assert!(matches!(m_whole.kind().unwrap(), MeasureKind::Other(_)));

        let m_slice = Measure::new("slice", 2.0);
        assert_eq!(
            m_slice.kind().unwrap(),
            MeasureKind::Other("slice".to_string())
        );
    }

    // ============================================================================
    // Serialization Tests
    // ============================================================================

    #[rstest]
    #[case::simple("cup", 2.0, None)]
    #[case::range("tablespoon", 1.0, Some(2.0))]
    #[case::custom("pinch", 1.0, None)]
    fn test_measure_serialization(
        #[case] unit: &str,
        #[case] value: f64,
        #[case] upper: Option<f64>,
    ) {
        let measure = match upper {
            Some(u) => Measure::with_range(unit, value, u),
            None => Measure::new(unit, value),
        };

        let json = serde_json::to_string(&measure).unwrap();
        let deserialized: Measure = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.values().0, value);
        assert_eq!(deserialized.values().1, upper);
    }

    #[test]
    fn test_measure_deserialization_unknown_unit() {
        let json = r#"{"unit":"weird_unit","value":1.0,"upper_value":null}"#;
        let measure: Measure = serde_json::from_str(json).unwrap();
        assert!(matches!(measure.unit(), Unit::Other(_)));
    }

    // ============================================================================
    // Display Tests
    // ============================================================================

    #[rstest]
    #[case::simple_cup("cup", 2.0, None, "2 cups")]
    #[case::simple_gram("g", 100.0, None, "100 g")]
    #[case::range("cup", 1.0, Some(2.0), "1 - 2 cups")]
    #[case::whole_unit("whole", 3.0, None, "3 whole")]
    #[case::whole_range("whole", 2.0, Some(4.0), "2 - 4 whole")]
    #[case::zero_upper_bound("cup", 1.0, Some(0.0), "1 cup")]
    fn test_measure_display(
        #[case] unit: &str,
        #[case] value: f64,
        #[case] upper: Option<f64>,
        #[case] expected: &str,
    ) {
        let measure = match upper {
            Some(u) => Measure::with_range(unit, value, u),
            None => Measure::new(unit, value),
        };
        assert_eq!(format!("{measure}"), expected);
    }
}
