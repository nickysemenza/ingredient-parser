use std::{fmt, str::FromStr};

use super::Unit;

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
    pub fn to_str(&self) -> &str {
        for (s, kind) in MEASURE_KIND_MAPPINGS {
            if self == kind {
                return s;
            }
        }
        "other"
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
        write!(f, "{self:?}")
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
    #[case::other(MeasureKind::Other("pinch".to_string()), "other")]
    #[case::nutrient(MeasureKind::Nutrient("g protein".to_string()), "other")]
    fn test_measure_kind_to_str(#[case] kind: MeasureKind, #[case] expected: &str) {
        assert_eq!(kind.to_str(), expected);
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
    #[case::weight(MeasureKind::Weight, "Weight")]
    #[case::volume(MeasureKind::Volume, "Volume")]
    #[case::nutrient(MeasureKind::Nutrient("g protein".to_string()), "Nutrient(\"g protein\")")]
    fn test_measure_kind_display(#[case] kind: MeasureKind, #[case] expected: &str) {
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
