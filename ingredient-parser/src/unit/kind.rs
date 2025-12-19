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

    #[test]
    fn test_measure_kind_unit() {
        // Test unit() for all kinds
        assert_eq!(MeasureKind::Weight.unit(), Unit::Gram);
        assert_eq!(MeasureKind::Volume.unit(), Unit::Milliliter);
        assert_eq!(MeasureKind::Money.unit(), Unit::Cent);
        assert_eq!(MeasureKind::Calories.unit(), Unit::KCal);
        assert_eq!(MeasureKind::Time.unit(), Unit::Second);
        assert_eq!(MeasureKind::Temperature.unit(), Unit::Fahrenheit);
        assert_eq!(MeasureKind::Length.unit(), Unit::Inch);
        assert_eq!(
            MeasureKind::Other("pinch".to_string()).unit(),
            Unit::Other("pinch".to_string())
        );
        // Test Nutrient.unit() - this is the missing coverage
        assert_eq!(
            MeasureKind::Nutrient("g protein".to_string()).unit(),
            Unit::Other("g protein".to_string())
        );
    }

    #[test]
    fn test_measure_kind_to_str() {
        assert_eq!(MeasureKind::Weight.to_str(), "weight");
        assert_eq!(MeasureKind::Volume.to_str(), "volume");
        assert_eq!(MeasureKind::Money.to_str(), "money");
        assert_eq!(MeasureKind::Calories.to_str(), "calories");
        assert_eq!(MeasureKind::Time.to_str(), "time");
        assert_eq!(MeasureKind::Temperature.to_str(), "temperature");
        assert_eq!(MeasureKind::Length.to_str(), "length");
        // Other and Nutrient fall through to "other"
        assert_eq!(MeasureKind::Other("pinch".to_string()).to_str(), "other");
        assert_eq!(
            MeasureKind::Nutrient("g protein".to_string()).to_str(),
            "other"
        );
    }

    #[test]
    fn test_measure_kind_from_str() {
        // Test standard kinds
        assert_eq!(
            MeasureKind::from_str("weight").unwrap(),
            MeasureKind::Weight
        );
        assert_eq!(
            MeasureKind::from_str("volume").unwrap(),
            MeasureKind::Volume
        );
        assert_eq!(MeasureKind::from_str("money").unwrap(), MeasureKind::Money);
        assert_eq!(
            MeasureKind::from_str("calories").unwrap(),
            MeasureKind::Calories
        );
        assert_eq!(MeasureKind::from_str("time").unwrap(), MeasureKind::Time);
        assert_eq!(
            MeasureKind::from_str("temperature").unwrap(),
            MeasureKind::Temperature
        );
        assert_eq!(
            MeasureKind::from_str("length").unwrap(),
            MeasureKind::Length
        );

        // Test case insensitivity
        assert_eq!(
            MeasureKind::from_str("WEIGHT").unwrap(),
            MeasureKind::Weight
        );
        assert_eq!(
            MeasureKind::from_str("Volume").unwrap(),
            MeasureKind::Volume
        );

        // Test nutrient prefix pattern
        assert_eq!(
            MeasureKind::from_str("nutrient:g protein").unwrap(),
            MeasureKind::Nutrient("g protein".to_string())
        );
        assert_eq!(
            MeasureKind::from_str("NUTRIENT:mg sodium").unwrap(),
            MeasureKind::Nutrient("mg sodium".to_string())
        );

        // Test unknown falls back to Other
        assert_eq!(
            MeasureKind::from_str("unknown").unwrap(),
            MeasureKind::Other("unknown".to_string())
        );
    }

    #[test]
    fn test_measure_kind_display() {
        // Test Display trait
        assert_eq!(format!("{}", MeasureKind::Weight), "Weight");
        assert_eq!(format!("{}", MeasureKind::Volume), "Volume");
        assert_eq!(
            format!("{}", MeasureKind::Nutrient("g protein".to_string())),
            "Nutrient(\"g protein\")"
        );
    }

    #[test]
    fn test_measure_kind_is_scalable() {
        // Scalable kinds
        assert!(MeasureKind::Weight.is_scalable());
        assert!(MeasureKind::Volume.is_scalable());
        assert!(MeasureKind::Other("pinch".to_string()).is_scalable());

        // Non-scalable kinds
        assert!(!MeasureKind::Time.is_scalable());
        assert!(!MeasureKind::Temperature.is_scalable());
        assert!(!MeasureKind::Calories.is_scalable());
        assert!(!MeasureKind::Money.is_scalable());
        assert!(!MeasureKind::Length.is_scalable());
        assert!(!MeasureKind::Nutrient("g protein".to_string()).is_scalable());
    }
}
