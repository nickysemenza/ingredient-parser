use crate::unit::singular;
use crate::unit::{Unit, kind::MeasureKind};
use crate::util::{format_quantity, num_without_zeroes};
use crate::{IngredientError, IngredientResult};
use num_rational::Rational64;
use num_traits::{CheckedAdd, ToPrimitive};
use serde::{Deserialize, Serialize, de::Deserializer, ser::Serializer};
use std::fmt;
use std::str::FromStr;
use tracing::debug;

/// Convert an `f64` quantity to an exact rational. Cooking fractions (½, ⅓, …)
/// and terminating decimals recover to their simple form via continued-fraction
/// approximation, so SAME-UNIT equality is exact (e.g. ⅓ == ⅓, not 0.333… ≈
/// 0.333…). NOTE: this exactness only holds within a single unit. Cross-unit
/// arithmetic (`normalize()` and the conversion graph) multiplies by `f64`
/// conversion factors and round-trips Rational64 → f64 → Rational64, so adding
/// measures of different units is approximate, not exact.
/// Input that `approximate_float` can't represent — non-finite, or a magnitude
/// beyond `i64` range — clamps to a sign-preserving extreme rather than
/// collapsing to zero, so a valid-but-enormous quantity stays enormous and
/// ordered instead of silently becoming 0 (which would corrupt the parse). NaN,
/// which shouldn't reach here once `finite_double` guards the number parsers,
/// maps to 0.
fn to_rational(value: f64) -> Rational64 {
    Rational64::approximate_float(value).unwrap_or_else(|| {
        if value.is_nan() {
            Rational64::from_integer(0)
        } else if value < 0.0 {
            Rational64::from_integer(i64::MIN)
        } else {
            Rational64::from_integer(i64::MAX)
        }
    })
}

/// Ensure a range reads low→high. A reversed range like "5 to 2 cups" is almost
/// certainly a transcription quirk, and downstream code assumes
/// `value <= upper_value`; swap so that invariant always holds. The upper-bound
/// -only form `(0.0, Some(upper))` ("up to 5") is already ordered, so it is left
/// untouched.
fn ordered_bounds(value: f64, upper_value: Option<f64>) -> (f64, Option<f64>) {
    match upper_value {
        Some(upper) if upper < value => (upper, Some(value)),
        other => (value, other),
    }
}

/// Best-effort `f64` view of a rational (for arithmetic, conversion, and the
/// `value()` public accessor).
fn to_f64(value: Rational64) -> f64 {
    value.to_f64().unwrap_or(0.0)
}

// Re-export conversion types and functions for backward compatibility
pub use super::conversion::{MeasureGraph, make_graph, print_graph};
// Crate-internal: the public entry point is the `Measure::convert_measure_via_mappings` method.
use super::conversion::convert_measure_via_mappings;

#[derive(Clone, PartialEq, PartialOrd, Debug, Serialize, Deserialize)]
pub struct Measure {
    #[serde(
        serialize_with = "serialize_unit",
        deserialize_with = "deserialize_unit"
    )]
    unit: Unit,
    // Stored as an exact rational so equality is exact; (de)serialized as f64 to
    // keep the JSON/wasm representation a plain number.
    #[serde(
        serialize_with = "serialize_rational",
        deserialize_with = "deserialize_rational"
    )]
    value: Rational64,
    #[serde(
        default,
        serialize_with = "serialize_rational_opt",
        deserialize_with = "deserialize_rational_opt"
    )]
    upper_value: Option<Rational64>,
}

/// Serialize Unit as its canonical string form (e.g., "cup", "g", "$")
fn serialize_unit<S: Serializer>(unit: &Unit, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&unit.to_str())
}

/// Deserialize Unit from a string
fn deserialize_unit<'de, D: Deserializer<'de>>(d: D) -> Result<Unit, D::Error> {
    let s = String::deserialize(d)?;
    // `Unit::from_str` is infallible (the fallback arm is unreachable);
    // `normalize` singularizes unknown units.
    Ok(Unit::from_str(&s).unwrap_or(Unit::Other(s)).normalize())
}

/// Serialize a rational quantity as a plain JSON number (f64).
fn serialize_rational<S: Serializer>(value: &Rational64, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_f64(to_f64(*value))
}

/// Deserialize a JSON number into an exact rational.
fn deserialize_rational<'de, D: Deserializer<'de>>(d: D) -> Result<Rational64, D::Error> {
    Ok(to_rational(f64::deserialize(d)?))
}

fn serialize_rational_opt<S: Serializer>(
    value: &Option<Rational64>,
    s: S,
) -> Result<S::Ok, S::Error> {
    value.map(to_f64).serialize(s)
}

fn deserialize_rational_opt<'de, D: Deserializer<'de>>(
    d: D,
) -> Result<Option<Rational64>, D::Error> {
    Ok(Option::<f64>::deserialize(d)?.map(to_rational))
}

// Multiplication factors for unit conversions
const TSP_TO_TBSP: f64 = 3.0;
// 1 fl oz = 2 tbsp = 6 tsp (the table below is teaspoons-per-from-unit)
const TSP_TO_FL_OZ: f64 = TSP_TO_TBSP * 2.0;
// Bridge between the two volume normalization bases (teaspoon for the US/spoon family,
// milliliter for the metric family). Fixed geometric ratio, density-independent:
// 1 US tsp = 4.92892 ml (keeps cup = 48 tsp = 236.59 ml consistent). Seeded into the
// conversion graph so US and metric volumes interconvert; see conversion.rs make_graph.
pub(crate) const TSP_TO_ML: f64 = 4.92892;
const G_TO_K: f64 = 1000.0;
const CUP_TO_QUART: f64 = 4.0;
const QUART_TO_GALLON: f64 = 4.0;
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
        from: Unit::Gallon,
        to_base: Unit::Teaspoon,
        factor: QUART_TO_GALLON * CUP_TO_QUART * TSP_TO_CUP, // 4 * 4 * 48 = 768 tsp/gal
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

/// Known nutrient names that follow the unit prefix, in their canonical
/// *singular* form — `is_nutrient_unit` singularizes the descriptor before
/// matching, so a unit that has been normalized ("g carbs" -> "g carb") is still
/// recognized. `carb` is the only entry that differs from its plural input
/// form; the rest are already singular.
static NUTRIENT_NAMES: &[&str] = &[
    "protein",
    "fat",
    "carb",
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
pub(crate) fn is_nutrient_unit(s: &str) -> bool {
    let mut parts = s.split_whitespace();
    let (Some(prefix), Some(name), None) = (parts.next(), parts.next(), parts.next()) else {
        return false; // require exactly two whitespace-separated tokens
    };
    let prefix = prefix.to_lowercase();
    // Singularize the descriptor so the normalized form ("carbs" -> "carb") still
    // matches — `make_graph`/`normalize` singularize units, so the classifier has
    // to agree or a normalized nutrient unit reads as `other:*` instead.
    let lowered = name.to_lowercase();
    let name = singular(&lowered);
    NUTRIENT_UNIT_PREFIXES.contains(&prefix.as_str()) && NUTRIENT_NAMES.contains(&name.as_ref())
}

impl Measure {
    pub(crate) fn new_with_upper(unit: Unit, value: f64, upper_value: Option<f64>) -> Measure {
        let (value, upper_value) = ordered_bounds(value, upper_value);
        Measure {
            unit,
            value: to_rational(value),
            upper_value: upper_value.map(to_rational),
        }
    }
    pub fn unit(&self) -> &Unit {
        &self.unit
    }
    /// Get the primary value of this measure
    pub fn value(&self) -> f64 {
        to_f64(self.value)
    }
    /// Get the upper value of this measure (for ranges like "2-3 cups")
    pub fn upper_value(&self) -> Option<f64> {
        self.upper_value.map(to_f64)
    }
    /// Normalize this measure to its base unit
    ///
    /// Converts units like cups to teaspoons, kg to grams, etc.
    /// Uses the NORMALIZATION_RULES table for conversion factors.
    pub(crate) fn normalize(&self) -> Measure {
        // Handle custom units - normalize the unit name (singularize)
        if let Unit::Other(x) = &self.unit {
            return Measure {
                unit: Unit::Other(singular(x).into_owned()),
                value: self.value,
                upper_value: self.upper_value,
            };
        }

        // Look up conversion rule in the table
        if let Some(rule) = find_normalization_rule(&self.unit) {
            return Measure {
                unit: rule.to_base.clone(),
                value: to_rational(self.value() * rule.factor),
                upper_value: self.upper_value().map(|x| to_rational(x * rule.factor)),
            };
        }

        // Unit is already a base unit, return as-is
        self.clone()
    }
    pub fn add(&self, b: Measure) -> IngredientResult<Measure> {
        debug!("adding {:?} to {:?}", self, b);

        let b_kind = b.kind();
        let self_kind = self.kind();

        if self_kind != b_kind {
            // A custom-unit rhs that doesn't match keeps self: unknown units must not
            // poison an otherwise-valid sum. Matching kinds — including identical
            // custom kinds like clove+clove or whole+whole — fall through and add.
            if matches!(b_kind, MeasureKind::Other(_)) {
                return Ok(self.clone());
            }
            return Err(IngredientError::MeasureError {
                operation: "add".to_string(),
                reason: format!(
                    "Cannot add measures of different kinds: {self_kind:?} and {b_kind:?}"
                ),
            });
        }
        let left = self.normalize();
        let right = b.normalize();

        // Exact rational add, but fall back to f64 if the i64 numerator/denominator
        // math would overflow. `num_rational`'s `+` panics (debug) / wraps (release)
        // on overflow; large quantities scaled by conversion factors can reach that,
        // so guard it to uphold the parser's "never panics" contract.
        let checked_add = |a: Rational64, b: Rational64| -> Rational64 {
            a.checked_add(&b)
                .unwrap_or_else(|| to_rational(to_f64(a) + to_f64(b)))
        };

        Ok(Measure {
            unit: left.unit.clone(),
            value: checked_add(left.value, right.value),
            upper_value: match (left.upper_value, right.upper_value) {
                (Some(a), Some(b)) => Some(checked_add(a, b)),
                (None, None) => None,
                (None, Some(b)) => Some(checked_add(left.value, b)),
                (Some(a), None) => Some(checked_add(a, right.value)),
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
    pub(crate) fn from_parts(unit: &str, value: f64, upper_value: Option<f64>) -> Measure {
        let normalized_unit = singular(unit);
        let unit =
            Unit::from_str(&normalized_unit).unwrap_or(Unit::Other(normalized_unit.into_owned()));

        let (value, upper_value) = ordered_bounds(value, upper_value);
        Measure {
            unit,
            value: to_rational(value),
            upper_value: upper_value.map(to_rational),
        }
    }
    /// Get the kind/category of this measurement (weight, volume, time, etc.).
    /// The category depends only on the unit, so this delegates to
    /// [`Unit::kind`] (which graph/unit-only callers can use without a value).
    pub fn kind(&self) -> MeasureKind {
        self.unit.kind()
    }

    pub fn denormalize(&self) -> Measure {
        let (u, f) = match &self.unit {
            Unit::Gram => (Unit::Gram, 1.0),
            Unit::Milliliter => (Unit::Milliliter, 1.0),
            Unit::Teaspoon => match self.value() {
                // only for these measurements to we convert to the best fit, others stay bare due to the nature of the values
                m if { m < 3.0 } => (Unit::Teaspoon, 1.0),
                m if { m < 12.0 } => (Unit::Tablespoon, TSP_TO_TBSP),
                m if { m < CUP_TO_QUART * TSP_TO_CUP } => (Unit::Cup, TSP_TO_CUP),
                _ => (Unit::Quart, CUP_TO_QUART * TSP_TO_CUP),
            },
            Unit::Cent => (Unit::Dollar, CENTS_TO_DOLLAR),
            Unit::KCal => (Unit::KCal, 1.0),
            Unit::Second => match self.value() {
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
            | Unit::Gallon
            | Unit::FluidOunce
            | Unit::Ounce
            | Unit::Pound
            | Unit::Dollar
            | Unit::Fahrenheit
            | Unit::Celsius
            | Unit::Whole
            | Unit::Minute
            | Unit::Hour
            | Unit::Day => return self.clone(),
        };
        Measure {
            unit: u,
            value: to_rational(self.value() / f),
            upper_value: self.upper_value().map(|x| to_rational(x / f)),
        }
    }

    /// Convert this measure to a target kind using user-provided mappings.
    ///
    /// This is a convenience wrapper around `convert_measure_via_mappings`.
    pub fn convert_measure_via_mappings(
        &self,
        target: MeasureKind,
        mappings: &[(Measure, Measure)],
    ) -> Option<Measure> {
        convert_measure_via_mappings(self, target, mappings)
    }
    /// Get the unit as a display string, pluralized when appropriate.
    ///
    /// For example, `Measure::new("cup", 2.0).unit_as_string()` returns `"cups"`.
    pub fn unit_as_string(&self) -> String {
        let unit_str = self.unit().to_str();
        let base = singular(&unit_str);
        if matches!(
            self.unit(),
            Unit::Cup | Unit::Second | Unit::Minute | Unit::Hour | Unit::Day
        ) && (self.value() > 1.0 || self.upper_value().unwrap_or(0.0) > 1.0)
        {
            let mut s = base.into_owned();
            s.push('s');
            s
        } else {
            base.into_owned()
        }
    }
}

impl fmt::Display for Measure {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let measure = self.denormalize();
        let value = measure.value();
        // Money renders symbol-first ("$5", "$2 - $4") instead of with a trailing
        // unit suffix, matching conventional currency formatting. `denormalize`
        // already folded `Cent` into `Dollar` (value in dollars), so the symbol is
        // always "$". Uses `num_without_zeroes` rather than `format_quantity` so
        // cents render as decimals ("$0.5") instead of vulgar fractions ("$½").
        if *measure.unit() == Unit::Dollar {
            let money = |v: f64| format!("${}", num_without_zeroes(v));
            return match measure.upper_value() {
                Some(u) if u != 0.0 && value == 0.0 => write!(f, "{}", money(u)),
                Some(u) if u != 0.0 => write!(f, "{} - {}", money(value), money(u)),
                _ => write!(f, "{}", money(value)),
            };
        }
        // `Unit::Whole` is the parser-internal sentinel for a bare count ("2 eggs"); it
        // renders as just the quantity. Serialization still emits "whole" via `to_str()`.
        // The suffix must come from the DENORMALIZED measure: `denormalize` can remap
        // the unit (48 tsp -> 1 cup, 7200 s -> 2 hours), and value/unit must agree.
        let suffix = if *measure.unit() == Unit::Whole {
            String::new()
        } else {
            format!(" {}", measure.unit_as_string())
        };
        if let Some(u) = measure.upper_value() {
            if u != 0.0 {
                if value == 0.0 {
                    // "up to X" case - just show the upper bound
                    write!(f, "{}{}", format_quantity(u), suffix)
                } else {
                    // Normal range "X - Y"
                    write!(
                        f,
                        "{} - {}{}",
                        format_quantity(value),
                        format_quantity(u),
                        suffix
                    )
                }
            } else {
                write!(f, "{}{}", format_quantity(value), suffix)
            }
        } else {
            write!(f, "{}{}", format_quantity(value), suffix)
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use rstest::rstest;

    // ============================================================================
    // Rational conversion + range-ordering invariants
    // ============================================================================

    /// A magnitude beyond `i64` range must clamp to a sign-preserving extreme,
    /// never collapse to 0 (which silently corrupted huge quantities before).
    #[test]
    fn test_to_rational_overflow_clamps_not_zero() {
        let huge = to_rational(1e30);
        assert_ne!(huge, Rational64::from_integer(0));
        assert!(to_f64(huge) > 0.0);
        assert_eq!(to_rational(-1e30), Rational64::from_integer(i64::MIN));
        // NaN has no magnitude, so it maps to 0.
        assert_eq!(to_rational(f64::NAN), Rational64::from_integer(0));
    }

    /// A reversed range ("5 to 2 cups") must be stored low→high so downstream
    /// code can rely on `value <= upper_value`.
    #[test]
    fn test_reversed_range_is_ordered() {
        let m = Measure::with_range("cup", 5.0, 2.0);
        assert_eq!(m.value(), 2.0);
        assert_eq!(m.upper_value(), Some(5.0));
        // An already-ordered range is untouched, and the upper-bound-only form
        // (0 lower) is left as-is.
        let ok = Measure::with_range("cup", 2.0, 3.0);
        assert_eq!((ok.value(), ok.upper_value()), (2.0, Some(3.0)));
        let upper_only = Measure::new_with_upper(Unit::Cup, 0.0, Some(5.0));
        assert_eq!(
            (upper_only.value(), upper_only.upper_value()),
            (0.0, Some(5.0))
        );
    }

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

    /// 1 gallon == 4 quarts: both normalize to 768 tsp, so a gallon-priced
    /// product reaches the standard volume graph instead of islanding.
    #[test]
    fn test_gallon_quart_roundtrip() {
        assert_eq!(
            Measure::new("gallon", 1.0).normalize(),
            Measure::new("quart", 4.0).normalize()
        );
        assert_eq!(
            Measure::new("gallon", 1.0).normalize(),
            Measure::new_with_upper(Unit::Teaspoon, 768.0, None)
        );
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
        assert_eq!(d.value(), expected_value);
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
        assert_eq!(*m.denormalize().unit(), expected);
    }

    #[rstest]
    #[case::small_sec(30.0, Unit::Second)]
    #[case::minute(120.0, Unit::Minute)]
    #[case::hour(7200.0, Unit::Hour)]
    #[case::day(90000.0, Unit::Day)]
    fn test_second_denormalize_unit(#[case] value: f64, #[case] expected: Unit) {
        let m = Measure::new_with_upper(Unit::Second, value, None);
        assert_eq!(*m.denormalize().unit(), expected);
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
    // "carbs" is the lone plural nutrient name; both it and the normalized
    // singular "carb" must be recognized so a unit isn't `other:*` after
    // singularization.
    #[case::g_carbs_plural("g carbs", true)]
    #[case::g_carb_singular("g carb", true)]
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
            m.kind(),
            MeasureKind::Nutrient(expected_nutrient.to_string())
        );
    }

    #[test]
    fn test_measure_kind_other_units() {
        let m_whole = Measure::new("whole", 1.0);
        assert!(matches!(m_whole.kind(), MeasureKind::Other(_)));

        let m_slice = Measure::new("slice", 2.0);
        assert_eq!(m_slice.kind(), MeasureKind::Other("slice".to_string()));
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

        assert_eq!(deserialized.value(), value);
        assert_eq!(deserialized.upper_value(), upper);
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
    // `Unit::Whole` renders bare (no " whole") — it's the parser-internal bare-count
    // sentinel. Serialization still emits "whole" (see core.rs `to_str` / `unit_as_string`).
    #[case::whole_unit("whole", 3.0, None, "3")]
    #[case::whole_single("whole", 1.0, None, "1")]
    #[case::whole_range("whole", 2.0, Some(4.0), "2 - 4")]
    #[case::zero_upper_bound("cup", 1.0, Some(0.0), "1 cup")]
    // Money renders symbol-first with a plain decimal (no vulgar fractions, no
    // trailing "$"), so it reads as conventional currency.
    #[case::money_dollars("$", 5.0, None, "$5")]
    #[case::money_cents("$", 0.01, None, "$0.01")]
    #[case::money_half("$", 0.5, None, "$0.5")]
    #[case::money_range("$", 2.0, Some(4.0), "$2 - $4")]
    // Display must pair the denormalized VALUE with the denormalized UNIT:
    // 48 tsp denormalizes to 1 cup, 7200 s to 2 hours.
    #[case::tsp_denormalizes_to_cup("tsp", 48.0, None, "1 cup")]
    #[case::seconds_denormalize_to_hours("second", 7200.0, None, "2 hours")]
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

    // ============================================================================
    // Add Tests (overflow-safety + same-unit exactness)
    // ============================================================================

    /// Adding two very large same-unit measures must not panic on `i64` overflow —
    /// `add` uses `checked_add` with an f64 fallback to uphold the "never panics"
    /// contract. The fallback loses exactness, which is acceptable for the rare
    /// overflow case.
    #[test]
    fn test_add_large_values_does_not_panic() {
        let big = 1e17_f64; // big enough that grams-scale rationals overflow i64 math
        let a = Measure::new("g", big);
        let b = Measure::new("g", big);
        let sum = a.add(b).unwrap();
        assert!(sum.value().is_finite());
        assert!(sum.value() > big);
    }

    /// 1 fl oz = 2 tbsp = 6 tsp, so 1 fl oz + 1 tbsp = 9 tsp (displays "3 tbsp").
    /// Pins the TSP_TO_FL_OZ factor, which was 2.0 (tbsp-per-fl-oz misapplied as
    /// tsp-per-fl-oz) — a silent 3x error in every fl-oz conversion.
    #[test]
    fn test_add_fl_oz_factor() {
        let sum = Measure::new("fl oz", 1.0)
            .add(Measure::new("tbsp", 1.0))
            .unwrap();
        assert_eq!(*sum.unit(), Unit::Teaspoon);
        assert_eq!(sum.value(), 9.0);
        assert_eq!(format!("{sum}"), "3 tbsp");
    }

    /// Adding two measures of the SAME custom kind must sum, not silently keep
    /// the left operand (the Other-kind early-return used to fire before the
    /// kind-equality check). Bare counts (whole) are Other("whole") and add too.
    #[rstest]
    #[case::custom_unit("clove", 1.0, 2.0, 3.0)]
    #[case::bare_count("whole", 2.0, 3.0, 5.0)]
    fn test_add_same_other_kind(
        #[case] unit: &str,
        #[case] a: f64,
        #[case] b: f64,
        #[case] expected: f64,
    ) {
        let sum = Measure::new(unit, a).add(Measure::new(unit, b)).unwrap();
        assert_eq!(sum.value(), expected);
    }

    /// Same-unit rational add stays exact: ⅓ cup + ⅓ cup == ⅔ cup, with no f64
    /// drift (the exactness guarantee scoped to same-unit arithmetic).
    #[test]
    fn test_add_same_unit_thirds_is_exact() {
        let third = 1.0 / 3.0;
        let a = Measure::new("cup", third);
        let b = Measure::new("cup", third);
        let sum = a.add(b).unwrap();
        // Compare exact rationals, not f64s: ⅓ + ⅓ == ⅔.
        let two_thirds = Measure::new("cup", 2.0 / 3.0).normalize();
        assert_eq!(sum.value, two_thirds.value);
    }
}
