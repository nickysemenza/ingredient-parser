use std::{borrow::Cow, fmt, str::FromStr};

use super::Unit;

// NOTE: deliberately NOT `#[non_exhaustive]`, unlike `Unit` (todo 009). The
// `ingredient-wasm` crate constructs `MeasureKind::Nutrient(_)` externally
// (ingredient-wasm/src/lib.rs:112,132); `#[non_exhaustive]` forbids external
// variant construction (E0639), so adding it would break the wasm build. Revisit
// if/when that construction moves behind a constructor fn in this crate.
#[derive(Clone, PartialEq, PartialOrd, Debug, Serialize, Deserialize)]
pub enum MeasureKind {
    Weight,
    Volume,
    Money,
    Calories,
    Other(String),
    Time,
    Temperature,
    Length,
    /// Target a specific nutrient unit like "g protein", "mg sodium"
    /// Used for direct conversions: "2 cups flour" → "X g protein"
    Nutrient(String),
}
impl MeasureKind {
    pub fn unit(&self) -> Unit {
        match self {
            MeasureKind::Weight => Unit::Gram,
            MeasureKind::Volume => Unit::Milliliter,
            MeasureKind::Money => Unit::Cent,
            MeasureKind::Calories => Unit::KCal,
            MeasureKind::Other(s) => Unit::Other(s.clone()),
            MeasureKind::Time => Unit::Second,
            MeasureKind::Temperature => Unit::Fahrenheit,
            MeasureKind::Length => Unit::Inch,
            MeasureKind::Nutrient(s) => Unit::Other(s.clone()),
        }
    }
    /// The kind's canonical string form, which inverts [`FromStr`]: the inner
    /// string of `Other`/`Nutrient` is carried as an `other:<s>`/`nutrient:<s>`
    /// prefix (both previously collapsed to a lossy bare `"other"`). Static
    /// variants borrow; the two parameterized ones allocate.
    pub fn to_str(&self) -> Cow<'_, str> {
        match self {
            MeasureKind::Other(s) => Cow::Owned(format!("other:{s}")),
            MeasureKind::Nutrient(s) => Cow::Owned(format!("nutrient:{s}")),
            _ => {
                for (s, kind) in MEASURE_KIND_MAPPINGS {
                    if self == kind {
                        return Cow::Borrowed(s);
                    }
                }
                Cow::Borrowed("other")
            }
        }
    }

    /// Returns whether this measure kind should scale when adjusting recipe quantities.
    ///
    /// Scalable: Weight, Volume, Other (pinch, clove, etc.)
    /// Not scalable: Time, Temperature, Calories, Money, Length, Nutrient
    pub fn is_scalable(&self) -> bool {
        matches!(
            self,
            MeasureKind::Weight | MeasureKind::Volume | MeasureKind::Other(_)
        )
    }
}

impl fmt::Display for MeasureKind {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // Delegate to the canonical `to_str` form so Display and the string
        // representation never diverge.
        write!(f, "{}", self.to_str())
    }
}

static MEASURE_KIND_MAPPINGS: &[(&str, MeasureKind)] = &[
    ("weight", MeasureKind::Weight),
    ("volume", MeasureKind::Volume),
    ("money", MeasureKind::Money),
    ("calories", MeasureKind::Calories),
    ("time", MeasureKind::Time),
    ("temperature", MeasureKind::Temperature),
    ("length", MeasureKind::Length),
];

impl FromStr for MeasureKind {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s_norm = s.to_lowercase();

        // Check for nutrient prefix pattern: "nutrient:g protein"
        if let Some(nutrient_unit) = s_norm.strip_prefix("nutrient:") {
            return Ok(MeasureKind::Nutrient(nutrient_unit.to_string()));
        }
        // Mirror `to_str`'s `other:<s>` form so the two round-trip.
        if let Some(other_unit) = s_norm.strip_prefix("other:") {
            return Ok(MeasureKind::Other(other_unit.to_string()));
        }

        for (str_repr, kind) in MEASURE_KIND_MAPPINGS {
            if s_norm == *str_repr {
                return Ok(kind.clone());
            }
        }
        Ok(MeasureKind::Other(s.to_string()))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use rstest::rstest;

    // ============================================================================
    // MeasureKind::unit() Tests
    // ============================================================================

    #[rstest]
    #[case::weight(MeasureKind::Weight, Unit::Gram)]
    #[case::volume(MeasureKind::Volume, Unit::Milliliter)]
    #[case::money(MeasureKind::Money, Unit::Cent)]
    #[case::calories(MeasureKind::Calories, Unit::KCal)]
    #[case::time(MeasureKind::Time, Unit::Second)]
    #[case::temperature(MeasureKind::Temperature, Unit::Fahrenheit)]
    #[case::length(MeasureKind::Length, Unit::Inch)]
    #[case::other(MeasureKind::Other("pinch".to_string()), Unit::Other("pinch".to_string()))]
    #[case::nutrient(MeasureKind::Nutrient("g protein".to_string()), Unit::Other("g protein".to_string()))]
    fn test_measure_kind_unit(#[case] kind: MeasureKind, #[case] expected: Unit) {
        assert_eq!(kind.unit(), expected);
    }

    // ============================================================================
    // MeasureKind::to_str() Tests
    // ============================================================================

    #[rstest]
    #[case::weight(MeasureKind::Weight, "weight")]
    #[case::volume(MeasureKind::Volume, "volume")]
    #[case::money(MeasureKind::Money, "money")]
    #[case::calories(MeasureKind::Calories, "calories")]
    #[case::time(MeasureKind::Time, "time")]
    #[case::temperature(MeasureKind::Temperature, "temperature")]
    #[case::length(MeasureKind::Length, "length")]
    #[case::other(MeasureKind::Other("pinch".to_string()), "other:pinch")]
    #[case::nutrient(MeasureKind::Nutrient("g protein".to_string()), "nutrient:g protein")]
    fn test_measure_kind_to_str(#[case] kind: MeasureKind, #[case] expected: &str) {
        assert_eq!(kind.to_str(), expected);
    }

    /// `to_str` must invert `from_str` for every variant, including the
    /// parameterized `Other`/`Nutrient` that previously lost their inner string.
    #[rstest]
    #[case::weight(MeasureKind::Weight)]
    #[case::length(MeasureKind::Length)]
    #[case::other(MeasureKind::Other("pinch".to_string()))]
    #[case::nutrient(MeasureKind::Nutrient("g protein".to_string()))]
    fn test_measure_kind_to_str_round_trips(#[case] kind: MeasureKind) {
        assert_eq!(MeasureKind::from_str(&kind.to_str()).unwrap(), kind);
    }

    // ============================================================================
    // MeasureKind::from_str() Tests
    // ============================================================================

    #[rstest]
    #[case::weight("weight", MeasureKind::Weight)]
    #[case::volume("volume", MeasureKind::Volume)]
    #[case::money("money", MeasureKind::Money)]
    #[case::calories("calories", MeasureKind::Calories)]
    #[case::time("time", MeasureKind::Time)]
    #[case::temperature("temperature", MeasureKind::Temperature)]
    #[case::length("length", MeasureKind::Length)]
    #[case::weight_uppercase("WEIGHT", MeasureKind::Weight)]
    #[case::volume_mixed_case("Volume", MeasureKind::Volume)]
    #[case::nutrient("nutrient:g protein", MeasureKind::Nutrient("g protein".to_string()))]
    #[case::nutrient_uppercase("NUTRIENT:mg sodium", MeasureKind::Nutrient("mg sodium".to_string()))]
    #[case::unknown("unknown", MeasureKind::Other("unknown".to_string()))]
    fn test_measure_kind_from_str(#[case] input: &str, #[case] expected: MeasureKind) {
        assert_eq!(MeasureKind::from_str(input).unwrap(), expected);
    }

    // ============================================================================
    // MeasureKind Display Tests
    // ============================================================================

    #[rstest]
    #[case::weight(MeasureKind::Weight, "weight")]
    #[case::volume(MeasureKind::Volume, "volume")]
    #[case::nutrient(MeasureKind::Nutrient("g protein".to_string()), "nutrient:g protein")]
    fn test_measure_kind_display(#[case] kind: MeasureKind, #[case] expected: &str) {
        // Display delegates to the canonical `to_str` form.
        assert_eq!(format!("{kind}"), expected);
    }

    // ============================================================================
    // MeasureKind::is_scalable() Tests
    // ============================================================================

    #[rstest]
    #[case::weight(MeasureKind::Weight, true)]
    #[case::volume(MeasureKind::Volume, true)]
    #[case::other(MeasureKind::Other("pinch".to_string()), true)]
    #[case::time(MeasureKind::Time, false)]
    #[case::temperature(MeasureKind::Temperature, false)]
    #[case::calories(MeasureKind::Calories, false)]
    #[case::money(MeasureKind::Money, false)]
    #[case::length(MeasureKind::Length, false)]
    #[case::nutrient(MeasureKind::Nutrient("g protein".to_string()), false)]
    fn test_measure_kind_is_scalable(#[case] kind: MeasureKind, #[case] expected: bool) {
        assert_eq!(kind.is_scalable(), expected);
    }
}
